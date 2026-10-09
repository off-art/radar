//! Фоновые сессии: каждый агент живёт в своём процессе-«хозяине» (`radar host`), который держит pty и экран.
//! Окно Radar подключается к нему по unix-сокету как клиент; закрыли окно — агент продолжает работать,
//! открыли снова — окно подключается обратно (`attach`).
//!
//! Протокол — кадры `[тип:1][длина:4 BE][данные]`.
//! Хозяин → клиент: `M` метаданные (JSON), `O` вывод агента (первый — снимок экрана), `X` код выхода (i32),
//! `H` событие хука (JSON), `B` «занято другим окном».
//! Клиент → хозяин: `A` подключиться, `I` ввод, `R` размер (rows u16, cols u16), `U` обновить метаданные (JSON),
//! `Q` завершить агента и хозяина (можно без `A` — так работает `radar stop`).
//! События хуков агент шлёт хозяину «голым» JSON (первый байт `{`), хозяин пересылает их окну кадром `H`.

use anyhow::{anyhow, bail, Context, Result};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

pub const MAX_FRAME: usize = 8 << 20;

/// Как запустить агента и что о нём помнить (хозяин получает это на stdin).
#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Spec {
    pub sock: PathBuf,
    pub shell: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
    pub env_remove: Vec<String>,
    pub meta: Meta,
}

/// Описание сессии: по нему окно собирает её заново при подключении.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Meta {
    pub id: u32,
    pub name: String,
    /// Отображаемое имя агента из конфига (`AgentDef.name`).
    pub agent: String,
    pub cwd: PathBuf,
    pub worktree: Option<PathBuf>,
    pub rows: u16,
    pub cols: u16,
    pub resume_id: Option<String>,
    pub muted: bool,
}

// ───────────── кадры ─────────────

pub fn write_frame(w: &mut impl Write, kind: u8, data: &[u8]) -> std::io::Result<()> {
    let mut buf = Vec::with_capacity(5 + data.len());
    buf.push(kind);
    buf.extend_from_slice(&(data.len() as u32).to_be_bytes());
    buf.extend_from_slice(data);
    w.write_all(&buf)?;
    w.flush()
}

/// Читает один кадр; `Ok(None)` — соединение закрыто.
pub fn read_frame(r: &mut impl Read) -> std::io::Result<Option<(u8, Vec<u8>)>> {
    let mut head = [0u8; 5];
    match r.read_exact(&mut head) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_be_bytes([head[1], head[2], head[3], head[4]]) as usize;
    if len > MAX_FRAME {
        return Err(std::io::Error::new(std::io::ErrorKind::InvalidData, "слишком большой кадр"));
    }
    let mut data = vec![0u8; len];
    r.read_exact(&mut data)?;
    Ok(Some((head[0], data)))
}

// ───────────── каталог сокетов ─────────────

pub fn run_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")).join(".radar").join("run")
}

/// Новый путь сокета хозяина.
pub fn new_sock_path(id: u32) -> PathBuf {
    let ms = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis()).unwrap_or(0);
    run_dir().join(format!("{ms}-{id}.sock"))
}

/// Сокеты всех хозяев (по возрастанию времени создания). Мёртвые сокеты удаляются.
pub fn live_sockets() -> Vec<PathBuf> {
    let Ok(rd) = std::fs::read_dir(run_dir()) else { return vec![] };
    let mut all: Vec<PathBuf> =
        rd.flatten().map(|e| e.path()).filter(|p| p.extension().map_or(false, |x| x == "sock")).collect();
    all.sort();
    all.retain(|p| {
        let ok = UnixStream::connect(p).is_ok();
        if !ok {
            let _ = std::fs::remove_file(p);
        }
        ok
    });
    all
}

// ───────────── хозяин ─────────────

struct Shared {
    /// Подключённое окно. Блокировка охватывает «обработать вывод + переслать», чтобы снимок экрана
    /// при подключении не дублировал и не терял байты.
    client: Mutex<Option<UnixStream>>,
    parser: Mutex<vt100::Parser>,
    meta: Mutex<Meta>,
    exit: Mutex<Option<i32>>,
}

/// Запуск хозяина: `radar host` (спецификация на stdin). Не возвращается, пока агент не будет закрыт.
pub fn run_host() -> Result<()> {
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input)?;
    let spec: Spec = serde_json::from_str(&input).context("неверная спецификация")?;
    let Meta { rows, cols, .. } = spec.meta;

    let pair = native_pty_system()
        .openpty(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 })
        .context("не удалось открыть pty")?;
    let mut cmd = CommandBuilder::new(&spec.shell);
    cmd.args(&spec.args);
    cmd.cwd(&spec.meta.cwd);
    for k in &spec.env_remove {
        cmd.env_remove(k);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = pair.slave.spawn_command(cmd).with_context(|| format!("не удалось запустить {}", spec.shell))?;
    drop(pair.slave);

    let mut reader = pair.master.try_clone_reader()?;
    let writer: Arc<Mutex<Box<dyn Write + Send>>> = Arc::new(Mutex::new(pair.master.take_writer()?));
    let master = Arc::new(Mutex::new(pair.master));
    let mut killer = child.clone_killer();

    if let Some(dir) = spec.sock.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let _ = std::fs::remove_file(&spec.sock);
    let listener = UnixListener::bind(&spec.sock)?;
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&spec.sock, std::fs::Permissions::from_mode(0o600));
    }

    let sh = Arc::new(Shared {
        client: Mutex::new(None),
        parser: Mutex::new(vt100::Parser::new(rows, cols, 0)),
        meta: Mutex::new(spec.meta.clone()),
        exit: Mutex::new(None),
    });

    // вывод агента: в эмулятор и окну
    {
        let sh = sh.clone();
        let writer = writer.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 16 * 1024];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let mut client = sh.client.lock().unwrap();
                        sh.parser.lock().unwrap().process(&buf[..n]);
                        answer_queries(&buf[..n], &writer, &sh.parser);
                        if let Some(c) = client.as_mut() {
                            if write_frame(c, b'O', &buf[..n]).is_err() {
                                *client = None;
                            }
                        }
                    }
                }
            }
        });
    }
    // завершение агента
    {
        let sh = sh.clone();
        std::thread::spawn(move || {
            let code = child.wait().map(|s| s.exit_code() as i32).unwrap_or(-1);
            let mut client = sh.client.lock().unwrap();
            *sh.exit.lock().unwrap() = Some(code);
            if let Some(c) = client.as_mut() {
                let _ = write_frame(c, b'X', &code.to_be_bytes());
            }
        });
    }

    for stream in listener.incoming().flatten() {
        let sh = sh.clone();
        let writer = writer.clone();
        let master = master.clone();
        let sock = spec.sock.clone();
        let mut killer = killer.clone_killer();
        std::thread::spawn(move || serve(stream, sh, writer, master, &mut *killer, &sock));
    }
    let _ = killer.kill();
    Ok(())
}

fn finish(sock: &Path, killer: &mut (dyn portable_pty::ChildKiller + Send + Sync)) -> ! {
    let _ = killer.kill();
    std::thread::sleep(Duration::from_millis(250)); // дать окну получить код выхода
    let _ = std::fs::remove_file(sock);
    std::process::exit(0);
}

fn serve(
    stream: UnixStream,
    sh: Arc<Shared>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    killer: &mut (dyn portable_pty::ChildKiller + Send + Sync),
    sock: &Path,
) {
    let mut stream = stream;
    let _ = stream.set_read_timeout(Some(Duration::from_secs(3)));
    // «голый» JSON от `radar hook`: первый байт `{`
    let mut first = [0u8; 1];
    if (&stream).peek_byte(&mut first).is_err() {
        return;
    }
    if first[0] == b'{' {
        let mut s = String::new();
        let _ = (&stream).take(1 << 20).read_to_string(&mut s);
        if let Some(c) = sh.client.lock().unwrap().as_mut() {
            let _ = write_frame(c, b'H', s.as_bytes());
        }
        return;
    }
    let Ok(Some((kind, _))) = read_frame(&mut stream) else { return };
    match kind {
        b'Q' => finish(sock, killer),
        b'A' => {}
        _ => return,
    }
    // подключение окна
    let _ = stream.set_read_timeout(None);
    {
        let mut client = sh.client.lock().unwrap();
        if client.is_some() {
            let _ = write_frame(&mut stream, b'B', b"");
            return;
        }
        let Ok(mut out) = stream.try_clone() else { return };
        let meta = sh.meta.lock().unwrap().clone();
        let snapshot = sh.parser.lock().unwrap().screen().state_formatted();
        if write_frame(&mut out, b'M', &serde_json::to_vec(&meta).unwrap_or_default()).is_err()
            || write_frame(&mut out, b'O', &snapshot).is_err()
        {
            return;
        }
        if let Some(code) = *sh.exit.lock().unwrap() {
            let _ = write_frame(&mut out, b'X', &code.to_be_bytes());
        }
        *client = Some(out);
    }
    // команды окна
    loop {
        match read_frame(&mut stream) {
            Ok(Some((b'I', data))) => {
                if let Ok(mut w) = writer.lock() {
                    let _ = w.write_all(&data);
                    let _ = w.flush();
                }
            }
            Ok(Some((b'R', d))) if d.len() == 4 => {
                let (rows, cols) = (u16::from_be_bytes([d[0], d[1]]), u16::from_be_bytes([d[2], d[3]]));
                if rows > 0 && cols > 0 {
                    let _guard = sh.client.lock().unwrap();
                    let _ = master.lock().unwrap().resize(PtySize { rows, cols, pixel_width: 0, pixel_height: 0 });
                    sh.parser.lock().unwrap().screen_mut().set_size(rows, cols);
                    let mut m = sh.meta.lock().unwrap();
                    m.rows = rows;
                    m.cols = cols;
                }
            }
            Ok(Some((b'U', d))) => {
                if let Ok(m) = serde_json::from_slice::<Meta>(&d) {
                    let mut cur = sh.meta.lock().unwrap();
                    cur.name = m.name;
                    cur.resume_id = m.resume_id;
                    cur.muted = m.muted;
                }
            }
            Ok(Some((b'Q', _))) => finish(sock, killer),
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => break,
        }
    }
    // окно отключилось — агент продолжает работать
    let mut client = sh.client.lock().unwrap();
    *client = None;
}

trait PeekByte {
    fn peek_byte(&self, buf: &mut [u8; 1]) -> std::io::Result<()>;
}

impl PeekByte for &UnixStream {
    fn peek_byte(&self, buf: &mut [u8; 1]) -> std::io::Result<()> {
        use std::os::fd::AsRawFd;
        let n = unsafe { libc::recv(self.as_raw_fd(), buf.as_mut_ptr() as *mut _, 1, libc::MSG_PEEK) };
        if n == 1 {
            Ok(())
        } else {
            Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "пусто"))
        }
    }
}

/// Отвечает на запросы терминалу (DA, DSR), на которые агенты иногда ждут ответ.
fn answer_queries(data: &[u8], writer: &Arc<Mutex<Box<dyn Write + Send>>>, parser: &Mutex<vt100::Parser>) {
    let contains = |needle: &[u8]| data.windows(needle.len()).any(|w| w == needle);
    let mut reply: Vec<u8> = Vec::new();
    if contains(b"\x1b[c") || contains(b"\x1b[0c") {
        reply.extend_from_slice(b"\x1b[?62;c");
    }
    if contains(b"\x1b[>c") || contains(b"\x1b[>0c") {
        reply.extend_from_slice(b"\x1b[>0;0;0c");
    }
    if contains(b"\x1b[5n") {
        reply.extend_from_slice(b"\x1b[0n");
    }
    if contains(b"\x1b[6n") {
        let (r, c) = parser.lock().unwrap().screen().cursor_position();
        reply.extend_from_slice(format!("\x1b[{};{}R", r + 1, c + 1).as_bytes());
    }
    if !reply.is_empty() {
        if let Ok(mut w) = writer.lock() {
            let _ = w.write_all(&reply);
            let _ = w.flush();
        }
    }
}

// ───────────── запуск хозяина из окна ─────────────

/// Запускает хозяина отдельным процессом (без терминала, переживёт закрытие окна) и ждёт, пока появится сокет.
pub fn launch(spec: &Spec) -> Result<()> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe()?;
    let logs = run_dir();
    std::fs::create_dir_all(&logs)?;
    let log = std::fs::OpenOptions::new().create(true).append(true).open(logs.join("host.log")).ok();
    let mut cmd = std::process::Command::new(exe);
    cmd.arg("host").stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::null());
    match log {
        Some(f) => cmd.stderr(f),
        None => cmd.stderr(std::process::Stdio::null()),
    };
    unsafe {
        cmd.pre_exec(|| {
            libc::setsid();
            Ok(())
        });
    }
    let mut child = cmd.spawn().context("не удалось запустить фонового хозяина агента")?;
    child.stdin.take().ok_or_else(|| anyhow!("нет stdin"))?.write_all(serde_json::to_string(spec)?.as_bytes())?;
    let start = Instant::now();
    loop {
        if spec.sock.exists() && UnixStream::connect(&spec.sock).is_ok() {
            return Ok(());
        }
        if let Ok(Some(st)) = child.try_wait() {
            bail!("хозяин агента завершился сразу ({st}); см. {}", logs.join("host.log").display());
        }
        if start.elapsed() > Duration::from_secs(8) {
            bail!("хозяин агента не запустился за 8 секунд");
        }
        std::thread::sleep(Duration::from_millis(25));
    }
}

/// Подключение окна: метаданные, поток кадров. `Ok(None)` — агент занят другим окном.
pub fn attach(sock: &Path) -> Result<Option<(Meta, UnixStream)>> {
    let mut s = UnixStream::connect(sock)?;
    s.set_read_timeout(Some(Duration::from_secs(3)))?;
    write_frame(&mut s, b'A', b"")?;
    match read_frame(&mut s)? {
        Some((b'M', d)) => {
            s.set_read_timeout(None)?;
            Ok(Some((serde_json::from_slice(&d)?, s)))
        }
        Some((b'B', _)) => Ok(None),
        _ => bail!("хозяин не ответил"),
    }
}

/// `radar stop`: завершает всех фоновых агентов. Возвращает, сколько остановлено.
pub fn stop_all() -> usize {
    let mut n = 0;
    for p in live_sockets() {
        if let Ok(mut s) = UnixStream::connect(&p) {
            if write_frame(&mut s, b'Q', b"").is_ok() {
                n += 1;
            }
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_roundtrip() {
        let mut buf = vec![];
        write_frame(&mut buf, b'O', b"hello").unwrap();
        write_frame(&mut buf, b'X', &7i32.to_be_bytes()).unwrap();
        let mut r = std::io::Cursor::new(buf);
        assert_eq!(read_frame(&mut r).unwrap(), Some((b'O', b"hello".to_vec())));
        assert_eq!(read_frame(&mut r).unwrap(), Some((b'X', 7i32.to_be_bytes().to_vec())));
        assert_eq!(read_frame(&mut r).unwrap(), None);
    }

    #[test]
    fn oversized_frame_rejected() {
        let mut head = vec![b'O'];
        head.extend_from_slice(&(u32::MAX).to_be_bytes());
        assert!(read_frame(&mut std::io::Cursor::new(head)).is_err());
    }

    #[test]
    fn meta_json() {
        let m = Meta {
            id: 3,
            name: "api #2".into(),
            agent: "Claude Code".into(),
            cwd: "/tmp".into(),
            worktree: None,
            rows: 30,
            cols: 100,
            resume_id: Some("abc".into()),
            muted: true,
        };
        let back: Meta = serde_json::from_slice(&serde_json::to_vec(&m).unwrap()).unwrap();
        assert_eq!(back, m);
    }
}
