//! `radar update`: самообновление из GitHub Releases (без внешних зависимостей: нужны только curl и tar).

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "off-art/radar";

/// Версия из адреса вида `https://github.com/o/r/releases/tag/v0.3.1` → `0.3.1`.
pub fn parse_tag(url: &str) -> Option<String> {
    let tag = url.trim().rsplit('/').next()?;
    let v = tag.strip_prefix('v').unwrap_or(tag);
    (v.split('.').count() >= 2 && v.split('.').all(|p| p.chars().all(|c| c.is_ascii_digit()) && !p.is_empty()))
        .then(|| v.to_string())
}

fn parts(v: &str) -> Vec<u64> {
    v.split('.').map(|p| p.parse().unwrap_or(0)).collect()
}

/// `a` новее `b`?
pub fn is_newer(a: &str, b: &str) -> bool {
    let (mut x, mut y) = (parts(a), parts(b));
    let n = x.len().max(y.len());
    x.resize(n, 0);
    y.resize(n, 0);
    x > y
}

fn target() -> Result<&'static str> {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => Ok("aarch64-apple-darwin"),
        ("macos", "x86_64") => Ok("x86_64-apple-darwin"),
        ("linux", "x86_64") => Ok("x86_64-unknown-linux-gnu"),
        ("linux", "aarch64") => Ok("aarch64-unknown-linux-gnu"),
        (os, arch) => bail!("обновление поддерживается на macOS и Linux (у вас {os}/{arch})"),
    }
}

/// Бинарник поставлен через Homebrew (лежит в Cellar, в том числе Linuxbrew)?
/// Тогда версией управляет brew, и самообновление его сломает.
pub fn is_brew_install(exe: &Path) -> bool {
    exe.components().any(|c| c.as_os_str() == "Cellar")
}

/// Бинарник поставлен системным пакетом (`.deb`): лежит в `/usr/`, обновлять его должен пакетный менеджер.
pub fn is_system_install(exe: &Path) -> bool {
    exe.starts_with("/usr/") && !exe.starts_with("/usr/local/")
}

fn repo() -> String {
    std::env::var("RADAR_REPO").unwrap_or_else(|_| REPO.to_string())
}

/// Последняя опубликованная версия (по редиректу `releases/latest`).
pub fn latest_version() -> Result<String> {
    let out = Command::new("curl")
        .args(["-fsSLI", "-o", "/dev/null", "-w", "%{url_effective}"])
        .arg(format!("https://github.com/{}/releases/latest", repo()))
        .output()
        .context("не найден curl")?;
    if !out.status.success() {
        bail!("не удалось связаться с GitHub — проверьте интернет");
    }
    parse_tag(&String::from_utf8_lossy(&out.stdout)).context("на GitHub пока нет опубликованных релизов")
}

fn run(cmd: &mut Command, what: &str) -> Result<()> {
    let st = cmd.status().with_context(|| format!("не удалось запустить: {what}"))?;
    if !st.success() {
        bail!("не получилось: {what}");
    }
    Ok(())
}

/// Ставит `new` на место `exe`: старый файл переименовывается, новый кладётся отдельным файлом
/// (перезапись работающего бинарника поверх убивает процесс на Apple Silicon).
fn replace(exe: &Path, new: &Path) -> Result<()> {
    let old = exe.with_extension("old");
    let _ = std::fs::remove_file(&old);
    std::fs::rename(exe, &old).with_context(|| format!("нет прав на запись в {}", exe.display()))?;
    let put = || -> Result<()> {
        std::fs::copy(new, exe)?;
        crate::ipc::make_executable(exe)?;
        if cfg!(target_os = "macos") {
            let _ = Command::new("codesign").args(["--force", "--sign", "-"]).arg(exe).output();
            let _ = Command::new("xattr").args(["-d", "com.apple.quarantine"]).arg(exe).output();
        }
        Ok(())
    };
    if let Err(e) = put() {
        let _ = std::fs::remove_file(exe);
        let _ = std::fs::rename(&old, exe);
        return Err(e.context("обновление отменено, старая версия на месте"));
    }
    let _ = std::fs::remove_file(&old);
    Ok(())
}

pub fn run_update(check_only: bool) -> Result<()> {
    let current = env!("CARGO_PKG_VERSION");
    println!("Текущая версия: {current}");
    let latest = latest_version()?;
    if !is_newer(&latest, current) {
        println!("Уже последняя версия.");
        return Ok(());
    }
    println!("Доступна версия {latest}.");
    if check_only {
        println!("Обновить: radar update");
        return Ok(());
    }
    let exe: PathBuf = std::env::current_exe()?.canonicalize()?;
    if is_brew_install(&exe) {
        println!("Radar установлен через Homebrew — обновите так:");
        println!("  brew update && brew upgrade off-art/radar/radar");
        return Ok(());
    }
    if is_system_install(&exe) {
        println!("Radar установлен системным пакетом (.deb) — обновите так:");
        println!(
            "  curl -fsSLO https://github.com/{}/releases/latest/download/radar_$(dpkg --print-architecture).deb",
            repo()
        );
        println!("  sudo apt install ./radar_$(dpkg --print-architecture).deb");
        return Ok(());
    }
    let t = target()?;
    let tmp = std::env::temp_dir().join(format!("radar-update-{}", std::process::id()));
    std::fs::create_dir_all(&tmp)?;
    let result = (|| -> Result<()> {
        let tgz = tmp.join("radar.tar.gz");
        println!("Скачиваю…");
        run(
            Command::new("curl")
                .args(["-fsSL", "-o"])
                .arg(&tgz)
                .arg(format!("https://github.com/{}/releases/download/v{latest}/radar-{t}.tar.gz", repo())),
            "скачать архив релиза",
        )?;
        run(Command::new("tar").arg("xzf").arg(&tgz).arg("-C").arg(&tmp), "распаковать архив")?;
        let new = tmp.join("radar");
        let ver = Command::new(&new).arg("--version").output().context("новый файл не запускается")?;
        if !String::from_utf8_lossy(&ver.stdout).contains(&latest) {
            bail!("в архиве не та версия — обновление отменено");
        }
        replace(&exe, &new)
    })();
    let _ = std::fs::remove_dir_all(&tmp);
    result?;
    println!("Готово: radar {latest}. Перезапустите открытые окна Radar.");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_parsing() {
        assert_eq!(parse_tag("https://github.com/off-art/radar/releases/tag/v0.3.1\n"), Some("0.3.1".into()));
        assert_eq!(parse_tag("https://github.com/off-art/radar/releases"), None);
    }

    #[test]
    fn brew_detection() {
        assert!(is_brew_install(Path::new("/opt/homebrew/Cellar/radar/0.6.8/bin/radar")));
        assert!(is_brew_install(Path::new("/home/linuxbrew/.linuxbrew/Cellar/radar/0.6.8/bin/radar")));
        assert!(!is_brew_install(Path::new("/Users/a/.local/bin/radar")));
    }

    #[test]
    fn system_install_detection() {
        assert!(is_system_install(Path::new("/usr/bin/radar")));
        assert!(!is_system_install(Path::new("/usr/local/bin/radar")));
        assert!(!is_system_install(Path::new("/home/a/.local/bin/radar")));
    }

    #[test]
    fn version_order() {
        assert!(is_newer("0.3.1", "0.3.0"));
        assert!(is_newer("0.10.0", "0.9.9"));
        assert!(is_newer("1.0", "0.9.9"));
        assert!(!is_newer("0.3.0", "0.3.0"));
        assert!(!is_newer("0.2.9", "0.3.0"));
    }

    #[test]
    fn replace_swaps_file_and_cleans_up() {
        let dir = crate::testutil::TempDir::new("upd");
        let (exe, new) = (dir.join("radar"), dir.join("new"));
        std::fs::write(&exe, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        replace(&exe, &new).unwrap();
        assert_eq!(std::fs::read_to_string(&exe).unwrap(), "new");
        assert!(!exe.with_extension("old").exists());
    }
}
