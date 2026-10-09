//! Действия с git над папкой агента: commit, push, слияние и удаление worktree.
//! Все функции блокирующие — интерфейс вызывает их из фонового потока.

use crate::git::exec;
use std::path::{Path, PathBuf};
use std::time::Duration;

const SHORT: Duration = Duration::from_secs(30);
const LONG: Duration = Duration::from_secs(90);

/// Создаёт отдельный worktree с новой веткой `radar/<агент>-<метка>`, чтобы агенты не мешали друг другу.
pub fn add_worktree(dir: &Path, agent_id: &str) -> Result<PathBuf, String> {
    let top = exec(dir, &["rev-parse", "--show-toplevel"], SHORT).map_err(|_| "папка не в git-репозитории")?;
    let top = PathBuf::from(top);
    let repo = top.file_name().map_or_else(|| "repo".into(), |s| s.to_string_lossy().to_string());
    let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs() % 100_000);
    let slug = format!("{agent_id}-{stamp}");
    let base = crate::paths::worktrees_dir();
    std::fs::create_dir_all(&base).map_err(|e| format!("не удалось создать {}: {e}", base.display()))?;
    let path = base.join(format!("{repo}-{slug}"));
    exec(&top, &["worktree", "add", "-b", &format!("radar/{slug}"), &path.to_string_lossy()], SHORT)?;
    Ok(path)
}

/// Добавляет все изменения и делает коммит.
pub fn commit(dir: &Path, message: &str) -> Result<String, String> {
    let message = message.trim();
    if message.is_empty() {
        return Err("пустое сообщение коммита".into());
    }
    exec(dir, &["add", "-A"], SHORT)?;
    let out = exec(dir, &["commit", "-m", message], SHORT)?;
    Ok(out.lines().next().unwrap_or("коммит создан").to_string())
}

/// Отправляет текущую ветку; при отсутствии upstream — `push -u origin <ветка>`.
pub fn push(dir: &Path) -> Result<String, String> {
    let branch = exec(dir, &["rev-parse", "--abbrev-ref", "HEAD"], SHORT)?;
    if branch == "HEAD" {
        return Err("отсоединённый HEAD: нет ветки для отправки".into());
    }
    let has_upstream = exec(dir, &["rev-parse", "--abbrev-ref", "@{u}"], SHORT).is_ok();
    if has_upstream {
        exec(dir, &["push"], LONG)?;
    } else {
        exec(dir, &["push", "-u", "origin", &branch], LONG)?;
    }
    Ok(format!("ветка «{branch}» отправлена"))
}

/// Основной репозиторий для worktree (папка, где лежит `.git`).
pub fn main_repo(worktree: &Path) -> Option<PathBuf> {
    let common = exec(worktree, &["rev-parse", "--git-common-dir"], SHORT).ok()?;
    let p = PathBuf::from(&common);
    let p = if p.is_absolute() { p } else { worktree.join(p) };
    let p = p.canonicalize().unwrap_or(p);
    (p.file_name()? == ".git").then(|| p.parent().map(Path::to_path_buf))?
}

/// Текущая ветка папки.
pub fn current_branch(dir: &Path) -> Option<String> {
    exec(dir, &["rev-parse", "--abbrev-ref", "HEAD"], SHORT).ok()
}

/// Вливает ветку агента в ту ветку, что сейчас открыта в основном репозитории.
/// При конфликте слияние откатывается.
pub fn merge_into_main(worktree: &Path, branch: &str) -> Result<String, String> {
    let main = main_repo(worktree).ok_or("не удалось найти основной репозиторий")?;
    let base = current_branch(&main).ok_or("не удалось определить основную ветку")?;
    if base == branch {
        return Err("основной репозиторий уже на этой ветке".into());
    }
    let msg = format!("Merge {branch}");
    match exec(&main, &["merge", "--no-ff", "-m", &msg, branch], LONG) {
        Ok(_) => Ok(format!("«{branch}» влита в «{base}»")),
        Err(e) => {
            let _ = exec(&main, &["merge", "--abort"], SHORT);
            let first = e.lines().next().unwrap_or("ошибка слияния");
            Err(format!("слияние не удалось и отменено: {first}"))
        }
    }
}

/// Удаляет worktree и, если ветка влита, саму ветку.
pub fn remove_worktree(worktree: &Path, branch: &str) -> Result<String, String> {
    let main = main_repo(worktree).ok_or("не удалось найти основной репозиторий")?;
    exec(&main, &["worktree", "remove", "--force", &worktree.to_string_lossy()], SHORT)?;
    if branch.is_empty() || branch == "HEAD" {
        return Ok("worktree удалён".into());
    }
    match exec(&main, &["branch", "-d", branch], SHORT) {
        Ok(_) => Ok(format!("worktree и ветка «{branch}» удалены")),
        Err(_) => Ok(format!("worktree удалён, ветка «{branch}» сохранена (не влита)")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{git, repo, TempDir};

    /// Репозиторий с одним коммитом (`a.txt`).
    fn setup(tag: &str) -> TempDir {
        let d = repo(tag);
        std::fs::write(d.join("a.txt"), "one\n").unwrap();
        git(&d, &["add", "."]);
        git(&d, &["commit", "-m", "first"]);
        d
    }

    #[test]
    fn commit_flow() {
        let d = setup("ops-commit");
        assert!(commit(&d, "  ").is_err());
        std::fs::write(d.join("b.txt"), "new\n").unwrap();
        let out = commit(&d, "add b").unwrap();
        assert!(out.contains("add b"), "{out}");
        assert!(crate::git::query(&d).unwrap().files == 0);
    }

    #[test]
    fn worktree_merge_and_remove() {
        let main = setup("ops-wt");
        let scratch = TempDir::new("ops-wt-tree");
        let wt = scratch.join("tree");
        git(&main, &["worktree", "add", "-b", "radar/x", &wt.to_string_lossy()]);
        assert_eq!(main_repo(&wt).unwrap().canonicalize().unwrap(), main.canonicalize().unwrap());

        std::fs::write(wt.join("feature.txt"), "f\n").unwrap();
        commit(&wt, "feature").unwrap();
        let msg = merge_into_main(&wt, "radar/x").unwrap();
        assert!(msg.contains("влита"), "{msg}");
        assert!(main.join("feature.txt").exists());

        let msg = remove_worktree(&wt, "radar/x").unwrap();
        assert!(msg.contains("удалены"), "{msg}");
        assert!(!wt.exists());
    }

    #[test]
    fn merge_conflict_is_aborted() {
        let main = setup("ops-conflict");
        let scratch = TempDir::new("ops-cf-tree");
        let wt = scratch.join("tree");
        git(&main, &["worktree", "add", "-b", "radar/c", &wt.to_string_lossy()]);
        std::fs::write(wt.join("a.txt"), "from worktree\n").unwrap();
        commit(&wt, "wt change").unwrap();
        std::fs::write(main.join("a.txt"), "from main\n").unwrap();
        commit(&main, "main change").unwrap();

        let err = merge_into_main(&wt, "radar/c").unwrap_err();
        assert!(err.contains("отменено"), "{err}");
        // основной репозиторий чист — слияние откатилось
        assert_eq!(crate::git::query(&main).unwrap().files, 0);
        // ветка не влита → remove сохраняет её
        let msg = remove_worktree(&wt, "radar/c").unwrap();
        assert!(msg.contains("сохранена"), "{msg}");
    }

    #[test]
    fn push_without_remote_reports_error() {
        let d = setup("ops-push");
        assert!(push(&d).is_err());
    }
}
