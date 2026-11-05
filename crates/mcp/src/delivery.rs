//! Files handed to the user: embedded MCP resources that chat clients (LibreChat, Claude Desktop)
//! offer for download.

use std::io::Read;
use std::path::{Path, PathBuf};

use cadcraft_engine::cmd::file::base64_encode;
use cadcraft_engine::file_access;
use serde_json::{Value, json};

/// The largest file `file_attach` hands over (and `drawing_create` publishes).
pub const MAX_ATTACH: u64 = 32 * 1024 * 1024;

/// DWG and DXF have no `application/*` type registered with IANA; theirs are `image/vnd.dwg` and
/// `image/vnd.dxf`, which chat clients take for pictures to show (LibreChat passes every `image/*`
/// resource to the model as an image). These are the common `application/*` names instead.
pub const DWG_MIME: &str = "application/acad";
pub const DXF_MIME: &str = "application/dxf";

/// The MIME type of an attachable file, by extension.
pub fn mime_of(path: &Path) -> Result<&'static str, String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or_default().to_ascii_lowercase();
    Ok(match ext.as_str() {
        "dwg" => DWG_MIME,
        "dxf" => DXF_MIME,
        "pdf" => "application/pdf",
        "svg" => "image/svg+xml",
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "csv" => "text/csv",
        "txt" => "text/plain",
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        _ => return Err(format!("file_attach can't attach .{ext} files (dwg, dxf, pdf, svg, png, jpeg, csv, txt, docx, xlsx, pptx)")),
    })
}

/// `{"type":"resource"}` content carrying a file: `file:///<percent-encoded name>`, its MIME type
/// and the bytes in base64.
pub fn resource(name: &str, mime: &str, data: &[u8]) -> Value {
    let mut encoded = String::new();
    for b in name.bytes() {
        if b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~') {
            encoded.push(char::from(b));
        } else {
            encoded.push_str(&format!("%{b:02X}"));
        }
    }
    json!({"type": "resource", "resource": {"uri": format!("file:///{encoded}"), "mimeType": mime, "blob": base64_encode(data)}})
}

/// Read a regular file (inside the working folder when one is in force), at most [`MAX_ATTACH`]
/// bytes: the read itself is bounded, not just the size the file system reports.
pub fn read_bounded(path: &str) -> Result<(PathBuf, Vec<u8>), String> {
    let file = file_access::open(Path::new(path)).map_err(|e| format!("{path}: {e}"))?;
    let meta = file.metadata().map_err(|e| format!("{path}: {e}"))?;
    if !meta.is_file() {
        return Err(format!("{path}: not a regular file"));
    }
    if meta.len() > MAX_ATTACH {
        return Err(format!("{path}: {} bytes is over the 32 MiB attachment limit", meta.len()));
    }
    let mut data = Vec::new();
    file.take(MAX_ATTACH + 1).read_to_end(&mut data).map_err(|e| format!("{path}: {e}"))?;
    if data.len() as u64 > MAX_ATTACH {
        return Err(format!("{path}: over the 32 MiB attachment limit"));
    }
    let abs = std::path::absolute(path).map_err(|e| format!("{path}: {e}"))?;
    Ok((abs, data))
}

/// `file_attach {path}`: a short JSON note plus the file as a downloadable resource.
pub fn attach(path: &str) -> Result<Value, String> {
    if path.is_empty() {
        return Err("file_attach needs a `path`".into());
    }
    let mime = mime_of(Path::new(path))?;
    let (abs, data) = read_bounded(path)?;
    let name = abs.file_name().and_then(|n| n.to_str()).ok_or("the file name is not valid UTF-8")?.to_string();
    let note = json!({"path": abs, "name": name, "bytes": data.len(), "mimeType": mime, "attached": true});
    Ok(json!({"content": [
        {"type": "text", "text": serde_json::to_string_pretty(&note).unwrap_or_default()},
        resource(&name, mime, &data),
    ]}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_percent_encoded_and_types_chosen_by_extension() {
        let r = resource("Plan Ä (2).dxf", DXF_MIME, b"0\nEOF\n");
        assert_eq!(r["resource"]["uri"], "file:///Plan%20%C3%84%20%282%29.dxf");
        assert_eq!(r["resource"]["mimeType"], "application/dxf");
        assert_eq!(r["resource"]["blob"], base64_encode(b"0\nEOF\n"));
        for (name, mime) in [
            ("a.DWG", "application/acad"),
            ("a.dxf", "application/dxf"),
            ("a.pdf", "application/pdf"),
            ("a.svg", "image/svg+xml"),
            ("a.png", "image/png"),
            ("a.csv", "text/csv"),
            ("a.docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
            ("a.xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
            ("a.pptx", "application/vnd.openxmlformats-officedocument.presentationml.presentation"),
        ] {
            assert_eq!(mime_of(Path::new(name)), Ok(mime), "{name}");
        }
        for bad in ["a.exe", "a", "a.dwt", ".bashrc"] {
            assert!(mime_of(Path::new(bad)).is_err(), "{bad}");
        }
        // Neither drawing type is `image/*`, which LibreChat shows as a picture.
        assert!(!DWG_MIME.starts_with("image/") && !DXF_MIME.starts_with("image/"));
    }
}
