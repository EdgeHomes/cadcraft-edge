//! The one folder an agent's commands may read and write (`cadcraft-cli mcp --root DIR`).
//!
//! While a [`Root`] is in force, every path a command reads goes through [`check_read`] and every
//! path it writes through [`check_write`]; the engine's file gateways ([`crate::capped_read`],
//! [`crate::atomic_write`] and the helpers here) call them, and a test keeps commands from reaching
//! for `std::fs` themselves.
//!
//! - The path is made absolute against the working directory (the server makes that the root, so
//!   relative paths are the root's). Its links are followed: for a file that doesn't exist yet,
//!   the links of its nearest existing folder, the rest of the path being plain names (`..` below
//!   a missing folder is refused, and so is a link that leads nowhere).
//! - The result must lie inside the root, compared by whole components (`/work2` isn't inside
//!   `/work`), without regard to case on Windows, where device names (`CON`, `NUL`…) are refused.
//! - Writes publish new files only: a file that exists is never replaced
//!   ([`replace_allowed`]); [`crate::atomic_write`] stages the bytes and hard-links them into
//!   place, which fails rather than overwrite.
//!
//! The root is in force in the whole process after [`confine_process`], and on one thread inside
//! [`confine`] (tests). Without a root nothing is checked.
//!
//! A path is checked, then opened: a process that can change the folders in between (swapping in
//! a link) can still win that race. The root keeps an agent to the files it was given; it doesn't
//! guard against other programs on the computer.

use std::cell::RefCell;
use std::ffi::OsString;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, OnceLock};

/// The folder automation is confined to.
#[derive(Debug)]
pub struct Root {
    /// The folder made absolute, links followed: what paths are compared with.
    resolved: PathBuf,
    /// The same without Windows' `\\?\` prefix, for people.
    shown: PathBuf,
}

impl Root {
    /// The existing folder `dir`; an error names the flag when it is missing or not a folder.
    pub fn new(dir: &Path) -> Result<Arc<Self>, String> {
        if dir.as_os_str().is_empty() {
            return Err("--root needs a folder".into());
        }
        let resolved = std::fs::canonicalize(dir).map_err(|e| format!("--root {} must be an existing folder: {e}", dir.display()))?;
        if !resolved.is_dir() {
            return Err(format!("--root {} is not a folder", dir.display()));
        }
        Ok(Arc::new(Self { shown: plain(resolved.clone()), resolved }))
    }

    /// The folder, absolute with its links followed.
    pub fn path(&self) -> &Path {
        &self.shown
    }

    /// May `path` be read or written? `Err` says why not.
    pub fn check(&self, path: &Path) -> Result<(), String> {
        let resolved = resolve(path)?;
        if inside(&resolved, &self.resolved) {
            Ok(())
        } else {
            Err(format!("path rejected: outside the working folder {}: {}", self.shown.display(), path.display()))
        }
    }
}

thread_local! {
    /// This thread's root ([`confine`]); `None`: the process's ([`confine_process`]).
    static SCOPE: RefCell<Option<Arc<Root>>> = const { RefCell::new(None) };
}

/// The root in force in every thread outside [`confine`].
static PROCESS: OnceLock<Arc<Root>> = OnceLock::new();

/// Put `root` in force in the whole process, for good. `false` when a root was in force already
/// (it stays as it was).
pub fn confine_process(root: Arc<Root>) -> bool {
    PROCESS.set(root).is_ok()
}

/// Puts the scope it replaced back when dropped (also when `f` panics).
struct Restore(Option<Arc<Root>>);

impl Drop for Restore {
    fn drop(&mut self) {
        let previous = self.0.take();
        SCOPE.with(|s| *s.borrow_mut() = previous);
    }
}

/// Run `f` with `root` in force on this thread.
pub fn confine<T>(root: &Arc<Root>, f: impl FnOnce() -> T) -> T {
    let _restore = Restore(SCOPE.with(|s| s.replace(Some(root.clone()))));
    f()
}

/// The root in force on this thread, if any.
pub fn current() -> Option<Arc<Root>> {
    SCOPE.with(|s| s.borrow().clone()).or_else(|| PROCESS.get().cloned())
}

/// May a write replace an existing file here? Not while a root is in force.
pub fn replace_allowed() -> bool {
    current().is_none()
}

/// May `path` be read here? Always, without a root in force.
pub fn check_read(path: &Path) -> Result<(), String> {
    current().map_or(Ok(()), |r| r.check(path))
}

/// May `path` be written here? Always, without a root in force.
pub fn check_write(path: &Path) -> Result<(), String> {
    current().map_or(Ok(()), |r| r.check(path))
}

fn denied(e: String) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::PermissionDenied, e)
}

/// [`std::fs::File::open`] for reading, checked.
pub fn open(path: &Path) -> std::io::Result<std::fs::File> {
    check_read(path).map_err(denied)?;
    std::fs::File::open(path)
}

/// [`std::fs::metadata`], checked.
pub fn metadata(path: &Path) -> std::io::Result<std::fs::Metadata> {
    check_read(path).map_err(denied)?;
    std::fs::metadata(path)
}

/// [`std::fs::read_dir`], checked.
pub fn read_dir(path: &Path) -> std::io::Result<std::fs::ReadDir> {
    check_read(path).map_err(denied)?;
    std::fs::read_dir(path)
}

/// [`std::fs::create_dir_all`], checked.
pub fn create_dir_all(path: &Path) -> std::io::Result<()> {
    check_write(path).map_err(denied)?;
    std::fs::create_dir_all(path)
}

/// [`std::fs::remove_file`], checked; never while a root is in force (nothing is replaced or
/// removed there).
pub fn remove_file(path: &Path) -> std::io::Result<()> {
    check_write(path).map_err(denied)?;
    if !replace_allowed() {
        return Err(denied(format!("{}: files are never removed in the working folder", path.display())));
    }
    std::fs::remove_file(path)
}

/// Open `path` to write text (LISP `open`): truncated or appended to, checked. While a root is in
/// force only a new file can be opened.
pub fn open_write(path: &Path, append: bool) -> std::io::Result<std::fs::File> {
    check_write(path).map_err(denied)?;
    let mut o = std::fs::OpenOptions::new();
    if replace_allowed() {
        o.create(true).write(true).append(append).truncate(!append);
    } else {
        o.create_new(true).write(true);
    }
    o.open(path)
}

/// `path` when nothing is there, else `stem (2).ext`, `stem (3).ext`…: a name to publish a new
/// file under, beside `path` (whose folder must exist and be writable here).
pub fn unused_name(path: &Path) -> Result<PathBuf, String> {
    let path = std::path::absolute(path).map_err(|e| format!("{}: {e}", path.display()))?;
    check_write(&path)?;
    let dir = path.parent().filter(|d| d.is_dir()).ok_or_else(|| format!("{}: the folder doesn't exist", path.display()))?;
    let stem = path.file_stem().and_then(|s| s.to_str()).filter(|s| !s.is_empty()).ok_or("the file needs a name")?;
    let ext = path.extension().and_then(|s| s.to_str()).map(|e| format!(".{e}")).unwrap_or_default();
    // `plan (2)` continues as `plan (3)`, not `plan (2) (2)`.
    let (stem, first) = match stem.strip_suffix(')').and_then(|s| s.rsplit_once(" (")) {
        Some((base, n)) if !base.is_empty() && n.len() <= 4 && n.bytes().all(|b| b.is_ascii_digit()) => (base, n.parse::<u32>().unwrap_or(1).max(1)),
        _ => (stem, 1),
    };
    for i in first..first.saturating_add(1000) {
        let candidate = if i == first { path.clone() } else { dir.join(format!("{stem} ({i}){ext}")) };
        if candidate.symlink_metadata().is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound) {
            return Ok(candidate);
        }
    }
    Err(format!("{}: no unused name left", path.display()))
}

/// `path` without Windows' `\\?\` prefix (which `canonicalize` adds), for people to read.
fn plain(path: PathBuf) -> PathBuf {
    if !cfg!(windows) {
        return path;
    }
    let text = path.to_string_lossy();
    if let Some(rest) = text.strip_prefix(r"\\?\UNC\") {
        return PathBuf::from(format!(r"\\{rest}"));
    }
    match text.strip_prefix(r"\\?\") {
        Some(rest) => PathBuf::from(rest),
        None => path,
    }
}

/// `path` made absolute with its links followed; for a path that doesn't exist (yet), its nearest
/// existing folder's links followed and the rest appended.
fn resolve(path: &Path) -> Result<PathBuf, String> {
    let shown = path.display();
    if path.as_os_str().is_empty() {
        return Err("path rejected: path is empty".into());
    }
    // Checked as given: making `…\NUL` absolute gives the device path `\\.\NUL` on Windows.
    if cfg!(windows) && path.components().any(|c| matches!(c, Component::Normal(n) if is_device_name(&n.to_string_lossy()))) {
        return Err(format!("path rejected: reserved device names are not allowed: {shown}"));
    }
    let absolute = std::path::absolute(path).map_err(|e| format!("path rejected: cannot resolve {shown}: {e}"))?;
    // The names below the nearest existing folder, last first.
    let mut missing: Vec<OsString> = vec![];
    let mut at = absolute.as_path();
    loop {
        match std::fs::canonicalize(at) {
            Ok(mut found) => {
                found.extend(missing.iter().rev());
                return Ok(found);
            }
            Err(e) => match std::fs::symlink_metadata(at) {
                Ok(m) if m.file_type().is_symlink() => return Err(format!("path rejected: a link that leads nowhere: {shown}")),
                // There, yet its real path can't be had (no permission): nothing to compare.
                Ok(_) => return Err(format!("path rejected: cannot resolve {shown}: {e}")),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(format!("path rejected: cannot resolve {shown}: {e}")),
            },
        }
        match (at.components().next_back(), at.parent()) {
            (Some(Component::Normal(name)), Some(parent)) => {
                missing.push(name.to_os_string());
                at = parent;
            }
            (Some(Component::ParentDir), _) => {
                return Err(format!("path rejected: parent traversal below a folder that doesn't exist: {shown}"));
            }
            // Down to the root (or drive) and it doesn't exist either.
            _ => return Err(format!("path rejected: cannot resolve {shown}: not found")),
        }
    }
}

/// Is `path` (resolved) `root` or inside it, component by component (without regard to case on
/// Windows, whose file systems ignore it)?
fn inside(path: &Path, root: &Path) -> bool {
    let mut path = path.components();
    root.components().all(|r| path.next().is_some_and(|p| same_component(p, r)))
}

fn same_component(a: Component, b: Component) -> bool {
    if a == b {
        return true;
    }
    if !cfg!(windows) {
        return false;
    }
    match (a.as_os_str().to_str(), b.as_os_str().to_str()) {
        (Some(a), Some(b)) => a.to_lowercase() == b.to_lowercase(),
        _ => false,
    }
}

/// A Windows device name (`CON`, `NUL`, `COM1`… also with an extension), which names no file.
pub fn is_device_name(component: &str) -> bool {
    let stem = component.split('.').next().unwrap_or(component).trim_end_matches(' ').to_ascii_uppercase();
    let numbered =
        |prefix: &str| stem.strip_prefix(prefix).is_some_and(|n| matches!(n, "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));
    matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CLOCK$" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder holding `root` (with `sub`, `a.dxf`) and `outside` (with `secret.dxf`).
    fn folders(name: &str) -> (PathBuf, Arc<Root>) {
        let base = std::env::temp_dir().join(format!("cc-file-access-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        for d in ["root/sub", "outside", "root2"] {
            std::fs::create_dir_all(base.join(d)).unwrap();
        }
        std::fs::write(base.join("root/a.dxf"), "x").unwrap();
        std::fs::write(base.join("outside/secret.dxf"), "x").unwrap();
        let root = Root::new(&base.join("root")).unwrap();
        (base, root)
    }

    #[test]
    fn paths_inside_the_root_pass_and_others_dont() {
        let (base, root) = folders("inside");
        assert_eq!(root.check(&base.join("root/a.dxf")), Ok(()));
        assert_eq!(root.check(&base.join("root")), Ok(()), "the root itself");
        assert_eq!(root.check(&base.join("root/sub/new.dxf")), Ok(()), "missing files are checked by their folder");
        assert_eq!(root.check(&base.join("root/new/deeper/x.png")), Ok(()), "and by their nearest folder");
        let e = root.check(&base.join("outside/secret.dxf")).unwrap_err();
        assert!(e.starts_with("path rejected: outside the working folder"), "{e}");
        assert!(root.check(&base.join("root2/b.dxf")).is_err(), "root2 is not inside root");
        assert!(root.check(Path::new("")).unwrap_err().contains("path is empty"));
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn parent_traversal_is_resolved_or_refused() {
        let (base, root) = folders("dotdot");
        assert!(root.check(&base.join("root/../outside/secret.dxf")).is_err(), "`..` out of the root");
        assert_eq!(root.check(&base.join("root/sub/../a.dxf")), Ok(()), "`..` that stays inside");
        let e = root.check(&base.join("root/nope/../../outside/x.dxf")).unwrap_err();
        assert!(e.contains("parent traversal below a folder that doesn't exist") || e.contains("outside the working folder"), "{e}");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn a_missing_root_or_a_file_is_an_error() {
        let (base, _) = folders("missing");
        assert!(Root::new(&base.join("nope")).unwrap_err().contains("must be an existing folder"));
        assert!(Root::new(&base.join("root/a.dxf")).unwrap_err().contains("is not a folder"));
        assert!(Root::new(Path::new("")).is_err());
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(unix)]
    #[test]
    fn links_are_followed_before_comparing() {
        use std::os::unix::fs::symlink;
        let (base, root) = folders("links");
        symlink(base.join("outside/secret.dxf"), base.join("root/leak.dxf")).unwrap();
        assert!(root.check(&base.join("root/leak.dxf")).unwrap_err().contains("outside the working folder"), "a file link out");
        symlink(base.join("outside"), base.join("root/away")).unwrap();
        assert!(root.check(&base.join("root/away/new.png")).unwrap_err().contains("outside"), "a folder link out");
        symlink(base.join("outside/gone.dxf"), base.join("root/dangling.dxf")).unwrap();
        assert!(root.check(&base.join("root/dangling.dxf")).unwrap_err().contains("a link that leads nowhere"));
        symlink(base.join("root/sub"), base.join("root/alias")).unwrap();
        assert_eq!(root.check(&base.join("root/alias/x.dxf")), Ok(()), "a link inside stays inside");
        symlink(base.join("root"), base.join("root-link")).unwrap();
        let linked = Root::new(&base.join("root-link")).unwrap();
        assert_eq!(linked.check(&base.join("root/a.dxf")), Ok(()), "a root given through a link is its folder");
        let _ = std::fs::remove_dir_all(base);
    }

    #[cfg(windows)]
    #[test]
    fn windows_ignores_case_and_refuses_devices() {
        let (base, root) = folders("case");
        assert_eq!(root.check(&base.join("ROOT/a.dxf")), Ok(()));
        assert_eq!(root.check(&base.join("Root/Sub/New.dxf")), Ok(()));
        assert!(root.check(&base.join("root/NUL")).unwrap_err().contains("reserved device names"));
        assert!(root.check(&base.join("root/com1.txt")).is_err());
        assert_eq!(root.check(&base.join("root/console.txt")), Ok(()));
        // Forward slashes and backslashes are both separators.
        let mixed = format!("{}/sub\\x.dxf", root.path().display());
        assert_eq!(root.check(Path::new(&mixed)), Ok(()));
        assert!(root.check(Path::new(r"C:\Windows\win.ini")).is_err());
        assert!(root.check(Path::new(r"\\server\share\x.dxf")).is_err(), "a UNC path outside");
        let _ = std::fs::remove_dir_all(base);
    }

    #[test]
    fn device_names() {
        for d in ["CON", "nul", "Com1", "LPT9.txt", "aux.dxf", "NUL ", "con.tar.gz"] {
            assert!(is_device_name(d), "{d}");
        }
        for f in ["console", "com10", "COM0", "lpt", "a.dxf", "nul-x"] {
            assert!(!is_device_name(f), "{f}");
        }
    }

    #[test]
    fn scopes_stay_on_their_thread_and_writes_are_new_only() {
        let (base, root) = folders("scopes");
        let outside = base.join("outside/secret.dxf");
        assert_eq!(check_read(&outside), Ok(()), "nothing is confined by default");
        assert!(replace_allowed());
        confine(&root, || {
            assert!(check_read(&outside).is_err());
            assert!(!replace_allowed());
            assert!(open_write(&base.join("root/a.dxf"), true).is_err(), "an existing file is never opened for writing");
            assert!(open_write(&base.join("root/new.txt"), false).is_ok());
            assert!(remove_file(&base.join("root/new.txt")).is_err());
            let o = outside.clone();
            assert!(std::thread::spawn(move || check_read(&o).is_ok()).join().unwrap(), "another thread isn't confined");
        });
        assert_eq!(check_read(&outside), Ok(()), "restored after confine");
        let r = std::panic::catch_unwind(|| confine(&root, || panic!("boom")));
        assert!(r.is_err());
        assert_eq!(check_read(&outside), Ok(()), "restored after a panic");
        let _ = std::fs::remove_dir_all(base);
    }

    /// Every file a command reads or writes by path goes through the gateways (`capped_read`,
    /// `atomic_write`, this module), which check the root. Engine code that reaches for `std::fs`
    /// itself would bypass them, so none may, except `autosave.rs`, whose folder the host sets
    /// (never a command's path), and test code.
    #[test]
    fn commands_do_not_touch_the_file_system_themselves() {
        fn walk(dir: &Path, files: &mut Vec<PathBuf>) {
            for entry in std::fs::read_dir(dir).unwrap() {
                let path = entry.unwrap().path();
                if path.is_dir() {
                    walk(&path, files);
                } else if path.extension().is_some_and(|e| e == "rs") {
                    files.push(path);
                }
            }
        }
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![];
        walk(&src, &mut files);
        assert!(files.iter().any(|f| f.ends_with("cmd/file.rs")), "{files:?}");
        let mut offenders = vec![];
        for path in files {
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let gateway = path.parent() == Some(src.as_path())
                && matches!(name, "file_access.rs" | "capped_read.rs" | "atomic_write.rs" | "autosave.rs" | "temp_dir.rs");
            if gateway || name.starts_with("tests") || path.components().any(|c| c.as_os_str() == "tests") {
                continue;
            }
            let text = std::fs::read_to_string(&path).unwrap();
            // Test modules come last in each file; a `#[cfg(test)]` import doesn't start them.
            let mut code = text.as_str();
            let mut from = 0;
            while let Some(i) = text[from..].find("#[cfg(test)]") {
                let at = from + i;
                let next = text[at + "#[cfg(test)]".len()..].trim_start();
                if next.starts_with("use ") || next.starts_with("pub use ") || next.starts_with("mod tests;") || next.starts_with("pub(crate) use ") {
                    from = at + 1;
                    continue;
                }
                code = &text[..at];
                break;
            }
            for (n, line) in code.lines().enumerate() {
                let line = line.trim_start();
                let words = [
                    "std::fs::read",
                    "std::fs::write",
                    "std::fs::metadata",
                    "std::fs::remove",
                    "std::fs::create_dir",
                    "std::fs::rename",
                    "std::fs::copy",
                    "std::fs::hard_link",
                    "File::open",
                    "File::create",
                    "OpenOptions",
                ];
                if !line.starts_with("//") && words.iter().any(|w| line.contains(w)) {
                    offenders.push(format!("{}:{}: {line}", path.display(), n + 1));
                }
            }
        }
        assert!(offenders.is_empty(), "use crate::file_access / capped_read / atomic_write: {offenders:#?}");
    }
}
