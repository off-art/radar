//! Платформенные мелочи под сокеты и процессы: на unix — стандартные средства, на Windows — их аналоги.
//! Unix-ветки здесь — тот же код, что раньше был в `host.rs`, `session.rs`, `hook.rs`; поведение не меняется.

use std::path::Path;
use std::process::Command;

#[cfg(unix)]
pub use std::os::unix::net::{UnixListener, UnixStream};
#[cfg(windows)]
pub use uds_windows::{UnixListener, UnixStream};

/// Доступ к сокету только владельцу (на Windows сокет лежит в профиле пользователя, отдельных прав не нужно).
pub fn restrict_to_owner(path: &Path) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(windows)]
    let _ = path;
}

/// Делает файл исполняемым (на Windows это не нужно).
pub fn make_executable(path: &Path) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
    }
    #[cfg(windows)]
    {
        let _ = path;
        Ok(())
    }
}

/// Отрывает запускаемый процесс от терминала Radar: на unix — собственная сессия (`setsid`),
/// на Windows — отдельный процесс без консоли.
pub fn detach(cmd: &mut Command) {
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // SAFETY: в `pre_exec` вызывается только async-signal-safe `setsid`.
        unsafe {
            cmd.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(DETACHED_PROCESS | CREATE_NO_WINDOW);
    }
}

/// Подсматривает первый байт входящих данных, не забирая его из потока.
pub fn peek_byte(stream: &UnixStream) -> std::io::Result<u8> {
    let mut buf = [0u8; 1];
    #[cfg(unix)]
    let n = {
        use std::os::fd::AsRawFd;
        // SAFETY: буфер живёт на время вызова, длина совпадает с переданной.
        unsafe { libc::recv(stream.as_raw_fd(), buf.as_mut_ptr() as *mut _, 1, libc::MSG_PEEK) as i64 }
    };
    #[cfg(windows)]
    let n = {
        use std::os::windows::io::AsRawSocket;
        #[link(name = "ws2_32")]
        extern "system" {
            fn recv(s: usize, buf: *mut u8, len: i32, flags: i32) -> i32;
        }
        const MSG_PEEK: i32 = 2;
        // SAFETY: сокет жив, пока жива ссылка; буфер на 1 байт.
        unsafe { recv(stream.as_raw_socket() as usize, buf.as_mut_ptr(), 1, MSG_PEEK) as i64 }
    };
    if n == 1 {
        Ok(buf[0])
    } else {
        Err(std::io::Error::new(std::io::ErrorKind::UnexpectedEof, "пусто"))
    }
}

/// Местное время (часы, минуты, секунды).
pub fn local_hms() -> (u32, u32, u32) {
    #[cfg(unix)]
    {
        // SAFETY: обнулённая `tm` — допустимое начальное значение, `localtime_r` только заполняет её.
        let t = unsafe { libc::time(std::ptr::null_mut()) };
        let mut tm: libc::tm = unsafe { std::mem::zeroed() };
        unsafe { libc::localtime_r(&t, &mut tm) };
        (tm.tm_hour as u32, tm.tm_min as u32, tm.tm_sec as u32)
    }
    #[cfg(windows)]
    {
        #[repr(C)]
        #[derive(Default)]
        struct SystemTime {
            year: u16,
            month: u16,
            day_of_week: u16,
            day: u16,
            hour: u16,
            minute: u16,
            second: u16,
            millis: u16,
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetLocalTime(t: *mut SystemTime);
        }
        let mut t = SystemTime::default();
        // SAFETY: структура совпадает с SYSTEMTIME и принадлежит нам.
        unsafe { GetLocalTime(&mut t) };
        (t.hour as u32, t.minute as u32, t.second as u32)
    }
}
