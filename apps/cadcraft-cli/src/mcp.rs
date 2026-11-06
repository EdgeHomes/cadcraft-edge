use super::args::{McpArgs, McpTarget, ToolsArgs};
use super::error::{CliError, Result};
use super::output::Streams;
use cadcraft_engine::file_access::{self, Root};
use serde_json::json;

pub(crate) fn mcp(a: &McpArgs) -> Result {
    let backend: Box<dyn cadcraft_mcp::Backend> = match a.target() {
        McpTarget::Connect { addr, lazy: true, launch } => Box::new(cadcraft_mcp::Remote::lazy(&addr, launch)),
        McpTarget::Connect { addr, .. } => match cadcraft_mcp::Remote::connect(&addr) {
            Ok(r) => Box::new(r),
            Err(source) => return Err(CliError::McpConnect { addr, source }),
        },
        McpTarget::Headless => Box::new(cadcraft_mcp::Headless::default()),
    };
    let dir = a.root.as_deref().map(home_relative);
    let root = match &dir {
        // Unrestricted: the working folder is only where relative paths and new drawings go.
        Some(dir) if !a.confine && !a.read_anywhere => {
            std::fs::create_dir_all(dir).map_err(|e| CliError::Root(format!("--root {}: the folder could not be made: {e}", dir.display())))?;
            std::env::set_current_dir(dir).map_err(|e| CliError::Root(format!("--root {}: {e}", dir.display())))?;
            eprintln!("cadcraft-cli: working folder {}; files elsewhere are reachable too", dir.display());
            None
        }
        // clap refuses --root with --connect: a running app's file access can't be confined.
        Some(dir) => {
            if a.create_root {
                if !dir.is_absolute() {
                    return Err(CliError::Root(format!("--root {}: the working folder must be an absolute path", dir.display())));
                }
                std::fs::create_dir_all(dir).map_err(|e| CliError::Root(format!("--root {}: the folder could not be made: {e}", dir.display())))?;
            }
            let root = if a.read_anywhere { Root::new_reading_anywhere(dir) } else { Root::new(dir) }.map_err(CliError::Root)?;
            // Relative paths are the working folder's.
            std::env::set_current_dir(root.path()).map_err(|e| CliError::Root(format!("--root {}: {e}", root.path().display())))?;
            // The settings folder is outside it: plotter configurations aren't read or written.
            cadcraft_engine::settings_dir::set(None);
            // Every thread of this process, not only the server's.
            file_access::confine_process(root.clone());
            let reads = if a.read_anywhere { "reads from anywhere, writes" } else { "files" };
            eprintln!("cadcraft-cli: {reads} confined to {}; saves create new files only", root.path().display());
            Some(root)
        }
        None => None,
    };
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let folder = dir.filter(|_| root.is_none());
    cadcraft_mcp::Server::new(backend).with_compact(a.compact).with_root(root).with_folder(folder).serve(stdin.lock(), stdout.lock())?;
    Ok(())
}

/// `~`, `~/x` and `~\x` are in the home folder (`HOME`, else `USERPROFILE` on Windows), so a
/// plugin can name `~/Documents/CADCraft` on every system.
fn home_relative(dir: &std::path::Path) -> std::path::PathBuf {
    let s = dir.to_string_lossy();
    let rest = match s.strip_prefix('~') {
        Some(r) if r.is_empty() || r.starts_with('/') || r.starts_with('\\') => r.trim_start_matches(['/', '\\']),
        _ => return dir.to_path_buf(),
    };
    match std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        Some(home) if !home.is_empty() => std::path::PathBuf::from(home).join(rest),
        _ => dir.to_path_buf(),
    }
}

/// `tools [--compact] [--json]`: the MCP tools the server offers, for checking a deployment.
pub(crate) fn tools(a: &ToolsArgs, io: &mut Streams) -> Result {
    let server = cadcraft_mcp::Server::new(Box::new(cadcraft_mcp::Headless::default())).with_compact(a.compact);
    let tools = server.tools();
    if a.json {
        let v = json!({"name": "cadcraft", "version": env!("CARGO_PKG_VERSION"), "compact": a.compact, "instructions": server.instructions(), "tools": tools});
        io.json(&v, false);
        return Ok(());
    }
    io.outln(format_args!("cadcraft-cli {} — {} MCP tools{}", env!("CARGO_PKG_VERSION"), tools.len(), if a.compact { " (compact)" } else { "" }));
    for t in &tools {
        let d = t["description"].as_str().unwrap_or_default();
        let first = d.split(". ").next().unwrap_or(d);
        io.outln(format_args!("  {:<16} {first}", t["name"].as_str().unwrap_or_default()));
    }
    Ok(())
}
