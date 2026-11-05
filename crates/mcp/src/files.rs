//! `find_files`: find a file by name on this computer, for a client (Claude Desktop) whose chat
//! attachments never reach the server: the model knows an attachment's name, not where the original
//! is. Reads go through the engine's checked gateway, so with a strict working folder only that
//! folder is searched. Not offered in `--compact` (no directory listing there).

use std::path::PathBuf;
use std::time::{Duration, Instant, UNIX_EPOCH};

use cadcraft_engine::file_access;
use serde_json::{Value, json};

/// Folders and files looked at in one search, at most.
const MAX_ENTRIES: usize = 200_000;
const MAX_DEPTH: usize = 12;
const MAX_TIME: Duration = Duration::from_secs(10);
const MAX_RESULTS: usize = 100;

/// The folders searched when the call names none: the working folder, then (when reads may come
/// from anywhere) Desktop, Downloads and Documents, then the whole home folder.
fn default_folders() -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::env::current_dir().into_iter().collect();
    if file_access::reads_anywhere() {
        let home = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from);
        if let Some(home) = home {
            v.extend(["Desktop", "Downloads", "Documents"].iter().map(|d| home.join(d)));
            v.push(home);
        }
    }
    v
}

/// Folders skipped while walking: hidden ones and bulky app or package folders no drawing is in.
fn skipped(name: &str) -> bool {
    name.starts_with('.') || matches!(name, "Library" | "node_modules" | "AppData" | "target" | "$RECYCLE.BIN")
}

/// `find_files {name, folder?, limit?}`: files whose name contains `name` (any case), newest first.
pub fn find(a: &Value) -> Result<Value, String> {
    let name = a
        .get("name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|n| !n.is_empty() && n.len() <= 255)
        .ok_or("find_files needs a `name` (1-255 characters)")?;
    let want = name.to_lowercase();
    let limit = a.get("limit").and_then(Value::as_u64).map_or(20, |l| (l as usize).clamp(1, MAX_RESULTS));
    let folders = match a.get("folder").and_then(Value::as_str) {
        Some(f) => vec![PathBuf::from(f)],
        None => default_folders(),
    };
    let start = Instant::now();
    let (mut seen, mut hits, mut done) = (0usize, Vec::new(), std::collections::BTreeSet::new());
    let mut stop = None;
    'outer: for top in folders {
        let mut queue = std::collections::VecDeque::from([(top, 0usize)]);
        while let Some((dir, depth)) = queue.pop_front() {
            // A folder reached again from a wider one (Downloads, then home) is walked once.
            let Ok(real) = std::fs::canonicalize(&dir) else { continue };
            if !done.insert(real) {
                continue;
            }
            let Ok(entries) = file_access::read_dir(&dir) else { continue };
            for e in entries.flatten() {
                seen += 1;
                if seen > MAX_ENTRIES || start.elapsed() > MAX_TIME {
                    stop = Some(if seen > MAX_ENTRIES { "entry limit" } else { "time limit" });
                    break 'outer;
                }
                let (Ok(kind), Some(n)) = (e.file_type(), e.file_name().to_str().map(str::to_string)) else { continue };
                // Links aren't followed: no loops, no detours outside the folders asked for.
                if kind.is_dir() && depth < MAX_DEPTH && !skipped(&n) {
                    queue.push_back((e.path(), depth + 1));
                } else if kind.is_file() && n.to_lowercase().contains(&want) {
                    let meta = e.metadata().ok();
                    let modified =
                        meta.as_ref().and_then(|m| m.modified().ok()).and_then(|t| t.duration_since(UNIX_EPOCH).ok()).map_or(0, |d| d.as_secs());
                    hits.push((modified, e.path(), meta.map_or(0, |m| m.len())));
                }
            }
        }
    }
    hits.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    hits.dedup_by(|a, b| a.1 == b.1);
    let count = hits.len();
    let files: Vec<Value> = hits.into_iter().take(limit).map(|(m, p, len)| json!({"path": p, "bytes": len, "modified": m})).collect();
    Ok(json!({"count": count, "files": files, "stopped": stop, "searched": seen}))
}
