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
    let root = match &a.root {
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
    cadcraft_mcp::Server::new(backend).with_compact(a.compact).with_root(root).serve(stdin.lock(), stdout.lock())?;
    Ok(())
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
