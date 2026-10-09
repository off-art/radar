//! Просмотр изменений агента: `git diff HEAD` + новые файлы, разобранные по файлам и строкам.

use crate::git;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Add,
    Del,
    Hunk,
    Ctx,
    Meta,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Line {
    pub kind: Kind,
    pub text: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileDiff {
    pub path: String,
    /// `M` изменён, `A` новый, `D` удалён, `R` переименован, `?` не отслеживается.
    pub status: char,
    pub added: u32,
    pub removed: u32,
    pub lines: Vec<Line>,
}

/// Состояние окна просмотра изменений.
pub struct View {
    pub title: String,
    pub dir: PathBuf,
    pub files: Vec<FileDiff>,
    pub sel: usize,
    pub scroll: usize,
}

impl View {
    pub fn file(&self) -> Option<&FileDiff> {
        self.files.get(self.sel)
    }

    pub fn total(&self) -> (u32, u32) {
        self.files.iter().fold((0, 0), |(a, r), f| (a + f.added, r + f.removed))
    }

    pub fn max_scroll(&self, height: usize) -> usize {
        self.file().map(|f| f.lines.len().saturating_sub(height)).unwrap_or(0)
    }

    pub fn select(&mut self, i: usize) {
        if i < self.files.len() {
            self.sel = i;
            self.scroll = 0;
        }
    }

    pub fn step_file(&mut self, d: isize) {
        let n = self.files.len() as isize;
        if n > 0 {
            self.select((self.sel as isize + d).rem_euclid(n) as usize);
        }
    }

    pub fn scroll_by(&mut self, d: isize, height: usize) {
        let max = self.max_scroll(height) as isize;
        self.scroll = (self.scroll as isize + d).clamp(0, max) as usize;
    }
}

const MAX_LINES_PER_FILE: usize = 5000;
const MAX_UNTRACKED_BYTES: u64 = 200_000;

/// Разбор unified diff (`git diff --no-color`).
pub fn parse(text: &str) -> Vec<FileDiff> {
    let mut files: Vec<FileDiff> = vec![];
    let mut in_hunk = false;
    for raw in text.lines() {
        if let Some(rest) = raw.strip_prefix("diff --git ") {
            // «a/путь b/путь»; берём вторую половину
            let path = rest.rsplit_once(" b/").map(|(_, p)| p.to_string()).unwrap_or_else(|| rest.to_string());
            files.push(FileDiff { path, status: 'M', added: 0, removed: 0, lines: vec![] });
            in_hunk = false;
            continue;
        }
        let Some(f) = files.last_mut() else { continue };
        if !in_hunk {
            if raw.starts_with("new file mode") {
                f.status = 'A';
            } else if raw.starts_with("deleted file mode") {
                f.status = 'D';
            } else if let Some(to) = raw.strip_prefix("rename to ") {
                f.status = 'R';
                f.path = to.to_string();
            } else if raw.starts_with("Binary files") || raw.starts_with("GIT binary patch") {
                f.lines.push(Line { kind: Kind::Meta, text: "двоичный файл".into() });
            } else if raw.starts_with("@@") {
                in_hunk = true;
                f.lines.push(Line { kind: Kind::Hunk, text: raw.to_string() });
            }
            continue;
        }
        if f.lines.len() >= MAX_LINES_PER_FILE {
            if f.lines.last().map(|l| l.kind) != Some(Kind::Meta) {
                f.lines.push(Line {
                    kind: Kind::Meta, text: "… дальше слишком много строк, показано не всё".into()
                });
            }
            // продолжаем считать +/−
            if raw.starts_with('+') {
                f.added += 1;
            } else if raw.starts_with('-') {
                f.removed += 1;
            }
            continue;
        }
        let (kind, text) = if raw.starts_with("@@") {
            (Kind::Hunk, raw)
        } else if let Some(t) = raw.strip_prefix('+') {
            f.added += 1;
            (Kind::Add, t)
        } else if let Some(t) = raw.strip_prefix('-') {
            f.removed += 1;
            (Kind::Del, t)
        } else if let Some(t) = raw.strip_prefix(' ') {
            (Kind::Ctx, t)
        } else if raw.starts_with('\\') {
            (Kind::Meta, raw)
        } else {
            (Kind::Ctx, raw)
        };
        f.lines.push(Line { kind, text: text.to_string() });
    }
    files
}

fn untracked(dir: &Path) -> Vec<FileDiff> {
    let Some(list) = git::run(dir, &["ls-files", "-o", "--exclude-standard", "-z"]) else {
        return vec![];
    };
    let mut out = vec![];
    for rel in list.split('\0').filter(|s| !s.is_empty()) {
        let p = dir.join(rel);
        let mut f = FileDiff { path: rel.to_string(), status: '?', added: 0, removed: 0, lines: vec![] };
        let meta = std::fs::metadata(&p);
        match meta {
            Ok(m) if m.is_file() && m.len() <= MAX_UNTRACKED_BYTES => match std::fs::read(&p) {
                Ok(bytes) if !bytes.contains(&0) => {
                    let text = String::from_utf8_lossy(&bytes);
                    f.lines.push(Line { kind: Kind::Hunk, text: "@@ новый файл @@".into() });
                    for l in text.lines().take(MAX_LINES_PER_FILE) {
                        f.lines.push(Line { kind: Kind::Add, text: l.to_string() });
                        f.added += 1;
                    }
                }
                _ => f.lines.push(Line { kind: Kind::Meta, text: "двоичный файл".into() }),
            },
            Ok(m) if m.is_file() => {
                f.lines.push(Line {
                    kind: Kind::Meta, text: "файл слишком большой для просмотра".into()
                })
            }
            _ => f.lines.push(Line { kind: Kind::Meta, text: "не удалось прочитать".into() }),
        }
        out.push(f);
    }
    out
}

/// Загружает изменения в папке. `Err` — не репозиторий или git недоступен.
pub fn load(dir: &Path) -> Result<Vec<FileDiff>, String> {
    git::run(dir, &["rev-parse", "--git-dir"]).ok_or("это не git-репозиторий")?;
    let limit = Duration::from_secs(8);
    let text = git::run_timeout(dir, &["diff", "HEAD", "--no-color", "--no-ext-diff", "-U3"], limit)
        .or_else(|| {
            // репозиторий без коммитов: HEAD ещё нет
            let a = git::run_timeout(dir, &["diff", "--cached", "--no-color", "--no-ext-diff", "-U3"], limit)?;
            let b = git::run_timeout(dir, &["diff", "--no-color", "--no-ext-diff", "-U3"], limit)?;
            Some(format!("{a}\n{b}"))
        })
        .ok_or("не удалось получить изменения (git не ответил)")?;
    let mut files = parse(&text);
    files.extend(untracked(dir));
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
diff --git a/src/a.rs b/src/a.rs
index 111..222 100644
--- a/src/a.rs
+++ b/src/a.rs
@@ -1,3 +1,4 @@
 fn main() {
-    old();
+    new();
+    more();
 }
\\ No newline at end of file
diff --git a/img.png b/img.png
new file mode 100644
index 000..333
Binary files /dev/null and b/img.png differ
diff --git a/old.txt b/new.txt
similarity index 90%
rename from old.txt
rename to new.txt
index 1..2 100644
--- a/old.txt
+++ b/new.txt
@@ -1 +1 @@
-x
+y
diff --git a/gone.rs b/gone.rs
deleted file mode 100644
index 4..0
--- a/gone.rs
+++ /dev/null
@@ -1,2 +0,0 @@
-a
-b
";

    #[test]
    fn parses_files_and_counts() {
        let f = parse(SAMPLE);
        assert_eq!(f.len(), 4);
        assert_eq!((f[0].path.as_str(), f[0].status, f[0].added, f[0].removed), ("src/a.rs", 'M', 2, 1));
        assert_eq!(f[0].lines[0].kind, Kind::Hunk);
        assert_eq!(f[0].lines[2].kind, Kind::Del);
        assert_eq!(f[0].lines[2].text, "    old();");
        assert_eq!(f[0].lines.last().unwrap().kind, Kind::Meta);
        assert_eq!((f[1].status, f[1].lines[0].text.as_str()), ('A', "двоичный файл"));
        assert_eq!((f[2].path.as_str(), f[2].status, f[2].added, f[2].removed), ("new.txt", 'R', 1, 1));
        assert_eq!((f[3].status, f[3].removed), ('D', 2));
    }

    #[test]
    fn empty_diff() {
        assert!(parse("").is_empty());
    }

    #[test]
    fn view_navigation() {
        let mut v = View { title: String::new(), dir: PathBuf::new(), files: parse(SAMPLE), sel: 0, scroll: 0 };
        assert_eq!(v.total(), (3, 4));
        v.step_file(-1);
        assert_eq!(v.sel, 3);
        v.step_file(1);
        assert_eq!(v.sel, 0);
        v.scroll_by(100, 3);
        assert_eq!(v.scroll, v.max_scroll(3));
        v.scroll_by(-100, 3);
        assert_eq!(v.scroll, 0);
    }

    #[test]
    fn loads_real_repository() {
        use std::process::{Command, Stdio};
        let d = std::env::temp_dir().join(format!("radar-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        let g = |args: &[&str]| {
            assert!(Command::new("git")
                .arg("-C")
                .arg(&d)
                .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "init.defaultBranch=main"])
                .args(args)
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status()
                .unwrap()
                .success());
        };
        g(&["init"]);
        std::fs::write(d.join("a.txt"), "one\ntwo\n").unwrap();
        g(&["add", "."]);
        g(&["commit", "-m", "first"]);
        std::fs::write(d.join("a.txt"), "one\nTWO\nthree\n").unwrap();
        std::fs::write(d.join("b.txt"), "new\n").unwrap();
        let files = load(&d).unwrap();
        assert_eq!(files.len(), 2);
        assert_eq!((files[0].path.as_str(), files[0].status, files[0].added, files[0].removed), ("a.txt", 'M', 2, 1));
        assert_eq!((files[1].path.as_str(), files[1].status, files[1].added), ("b.txt", '?', 1));
        let _ = std::fs::remove_dir_all(&d);
    }
}
