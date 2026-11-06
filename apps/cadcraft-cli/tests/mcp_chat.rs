//! The chat deployment of `cadcraft-cli mcp` (LibreChat, Claude Desktop), end to end over stdio:
//! `drawing_create`, uploads edited and saved under a new name, `file_attach`, `--root`
//! confinement and new-file-only writes, `--compact`, and the `tools` command the packages are
//! checked with. Every fixture is made in code by the binary itself.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{Value, json};

const BIN: &str = env!("CARGO_BIN_EXE_cadcraft-cli");

/// A running `cadcraft-cli mcp`.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    id: u64,
}

impl Mcp {
    fn start(args: &[&str]) -> Mcp {
        let mut child = Command::new(BIN)
            .arg("mcp")
            .args(args)
            .env("CADCRAFT_CONFIG_DIR", "")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        let mut m = Mcp { child, stdin, stdout, id: 0 };
        let init = m.rpc("initialize", json!({"protocolVersion": "2025-06-18"}));
        assert_eq!(init["result"]["serverInfo"]["name"], "cadcraft", "{init}");
        m
    }

    fn rooted(root: &Path, compact: bool) -> Mcp {
        let r = root.to_str().unwrap();
        if compact { Mcp::start(&["--compact", "--confine", "--root", r]) } else { Mcp::start(&["--confine", &format!("--root={r}")]) }
    }

    fn rpc(&mut self, method: &str, params: Value) -> Value {
        self.id += 1;
        writeln!(self.stdin, "{}", json!({"jsonrpc": "2.0", "id": self.id, "method": method, "params": params})).unwrap();
        self.stdin.flush().unwrap();
        let mut line = String::new();
        self.stdout.read_line(&mut line).unwrap();
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("{e}: {line}"))
    }

    /// The raw `tools/call` result (`content`, `isError`).
    fn call(&mut self, tool: &str, args: Value) -> Value {
        let r = self.rpc("tools/call", json!({"name": tool, "arguments": args}));
        r.get("result").cloned().unwrap_or_else(|| json!({"protocolError": r["error"]}))
    }

    /// A tool that must succeed: its first content block's JSON.
    fn ok(&mut self, tool: &str, args: Value) -> Value {
        let r = self.call(tool, args);
        assert!(r["isError"] != json!(true) && r.get("protocolError").is_none(), "{tool} failed: {r}");
        serde_json::from_str(r["content"][0]["text"].as_str().unwrap_or("null")).unwrap_or(Value::Null)
    }

    /// A tool that must fail: its message.
    fn err(&mut self, tool: &str, args: Value) -> String {
        let r = self.call(tool, args);
        assert!(r["isError"] == json!(true) || r.get("protocolError").is_some(), "{tool} should have failed: {r}");
        r.to_string()
    }
}

impl Drop for Mcp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn folder(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("cc-mcp-chat-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    // The real path: macOS's temp folder is behind a link.
    std::fs::canonicalize(d).unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().file_name().to_string_lossy().into_owned()).collect();
    v.sort();
    v
}

fn decode(b64: &str) -> Vec<u8> {
    const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = vec![];
    let (mut acc, mut bits) = (0u32, 0);
    for c in b64.bytes().filter(|c| *c != b'=') {
        acc = (acc << 6) | A.iter().position(|a| *a == c).unwrap() as u32;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    out
}

/// The embedded file of a tool result: (uri, mime type, bytes).
fn resource(r: &Value) -> (String, String, Vec<u8>) {
    let c = r["content"].as_array().unwrap().iter().find(|c| c["type"] == "resource").unwrap_or_else(|| panic!("no resource: {r}"));
    let res = &c["resource"];
    (res["uri"].as_str().unwrap().into(), res["mimeType"].as_str().unwrap().into(), decode(res["blob"].as_str().unwrap()))
}

fn plan() -> Value {
    json!({
        "path": "plan.dxf", "units": "mm",
        "layers": [{"name": "Walls", "color": 7, "lineweight": 0.5}, {"name": "Doors", "color": "yellow"}, {"name": "Furniture", "color": 3, "linetype": "DASHED"}],
        "blocks": [{"name": "Chair", "entities": [{"type": "rectangle", "corner1": [-250, -250], "corner2": [250, 250], "layer": "Furniture"}]}],
        "entities": [
            {"type": "rectangle", "corner1": [0, 0], "corner2": [10000, 6000], "layer": "Walls"},
            {"type": "line", "points": [[6000, 0], [6000, 6000]], "layer": "Walls"},
            {"type": "arc", "center": [6000, 2000], "radius": 900, "start": 90, "end": 180, "layer": "Doors"},
            {"type": "circle", "center": [2000, 4000], "radius": 300},
            {"type": "hatch", "points": [[0, 0], [10000, 0], [10000, -200], [0, -200]]},
            {"type": "text", "at": [3000, 3000], "text": "LIVING ROOM", "justify": "MC"},
            {"type": "text", "at": [8000, 3000], "text": "BEDROOM", "justify": "MC"},
            {"type": "mtext", "at": [200, 5800], "text": "Ground floor\nScale 1:50", "width": 3000},
            {"type": "dimension", "p1": [0, 0], "p2": [10000, 0], "at": [5000, -1000]},
            {"type": "dimension", "kind": "radius", "center": [6000, 2000], "point": [5100, 2000]},
            {"type": "insert", "name": "Chair", "at": [2000, 2000]}
        ]
    })
}

#[test]
fn drawing_create_makes_a_valid_file_that_reopens_and_attaches() {
    let root = folder("create");
    let mut m = Mcp::rooted(&root, false);
    let r = m.call("drawing_create", plan());
    assert_ne!(r["isError"], json!(true), "{r}");
    let summary: Value = serde_json::from_str(r["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(summary["dropped"], json!([]), "{summary}");
    assert_eq!(summary["entities"], 12, "11 entities and the block's rectangle: {summary}");
    assert_eq!(summary["plotScale"], "1:50");
    assert_eq!(summary["textHeight"], 125.0, "2.5 mm at 1:50");
    // A relative path lands in the working folder.
    assert_eq!(names(&root), ["plan.dxf"]);
    let on_disk = std::fs::read(root.join("plan.dxf")).unwrap();
    let (uri, mime, bytes) = resource(&r);
    assert_eq!((uri.as_str(), mime.as_str()), ("file:///plan.dxf", "application/dxf"));
    assert_eq!(bytes, on_disk, "the attachment is the saved file");
    // It reopens as the same drawing.
    m.ok("open", json!({"path": "plan.dxf"}));
    let d = m.ok("inspect_drawing", json!({}));
    for (kind, n) in [("Text", 2), ("MText", 1), ("Dimension", 2), ("Hatch", 1), ("Block Reference", 1), ("Arc", 1), ("Circle", 1), ("Line", 1)] {
        assert_eq!(d["counts"][kind], n, "{kind}: {}", d["counts"]);
    }
    let layers = d["layers"].to_string();
    for l in ["Walls", "Doors", "Furniture", "Text", "Dimensions", "Hatch"] {
        assert!(layers.contains(l), "{l}: {layers}");
    }
    let attached = m.call("file_attach", json!({"path": "plan.dxf"}));
    assert_eq!(resource(&attached).2, on_disk);
    // DWG too, through the conversion bridge.
    let mut dwg = plan();
    dwg["path"] = json!("plan.dwg");
    let r = m.call("drawing_create", dwg);
    assert_ne!(r["isError"], json!(true), "{r}");
    let (uri, mime, bytes) = resource(&r);
    assert_eq!((uri.as_str(), mime.as_str()), ("file:///plan.dwg", "application/acad"));
    assert!(bytes.starts_with(b"AC10"), "a DWG");
    m.ok("open", json!({"path": "plan.dwg"}));
    assert_eq!(m.ok("inspect_drawing", json!({}))["counts"]["Dimension"], 2);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn absolute_upload_edit_save_as_attach_keeps_the_original_and_untouched_entities() {
    let root = folder("upload");
    // The upload, made by an unconfined server, as a chat client would drop it in the folder.
    let upload = root.join("upload plan.dxf");
    let mut maker = Mcp::start(&[]);
    let mut p = plan();
    p["path"] = json!(upload.to_str().unwrap());
    maker.ok("drawing_create", p);
    drop(maker);
    let original = std::fs::read(&upload).unwrap();

    let mut m = Mcp::rooted(&root, true);
    m.ok("open", json!({"path": upload.to_str().unwrap()}));
    let before = m.ok("query_entities", json!({}));
    let bedroom = m.ok("find_text", json!({"contains": "bedroom"}));
    let handle = bedroom["texts"][0]["handle"].as_str().unwrap_or_else(|| panic!("{bedroom}")).to_string();
    m.ok("execute", json!({"command": "textedit", "params": {"handle": handle, "text": "MAIN BEDROOM"}}));
    let r = m.ok("command_line", json!({"text": "-layer m Notes c 1  "}));
    assert!(r.get("error").is_none(), "{r}");
    // Saving over the upload is refused; a save without a path publishes a new name beside it.
    assert!(m.err("save", json!({"path": upload.to_str().unwrap()})).contains("never replaced"));
    let saved = m.ok("save", json!({}));
    let new = root.join("upload plan (2).dxf");
    assert_eq!(Path::new(saved["path"].as_str().unwrap()), new, "{saved}");
    let r = m.call("file_attach", json!({"path": "upload plan (2).dxf"}));
    let (uri, _, bytes) = resource(&r);
    assert_eq!(uri, "file:///upload%20plan%20%282%29.dxf");
    assert_eq!(bytes, std::fs::read(&new).unwrap());
    assert_eq!(std::fs::read(&upload).unwrap(), original, "the upload is untouched");

    // Reopened: the edit is there, and everything else kept its handle, type and layer.
    m.ok("open", json!({"path": "upload plan (2).dxf"}));
    assert_eq!(m.ok("find_text", json!({"contains": "main bedroom"}))["texts"][0]["handle"], json!(handle));
    let after = m.ok("query_entities", json!({}));
    let key = |e: &Value| (e["handle"].to_string(), e["type"].to_string(), e["layer"].to_string());
    let a: Vec<_> = after["entities"].as_array().unwrap().iter().map(key).collect();
    for e in before["entities"].as_array().unwrap() {
        assert!(a.contains(&key(e)), "lost {e}");
    }
    let d = m.ok("inspect_drawing", json!({}));
    let layers = d["layers"].to_string();
    for l in ["Walls", "Doors", "Furniture", "Notes"] {
        assert!(layers.contains(l), "{l}: {layers}");
    }
    assert!(d["blocks"].to_string().contains("Chair"));
    assert!(d["dimStyles"].to_string().contains("CADCraft") || d.to_string().contains("CADCraft"), "the dimension style is kept");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn compact_lists_the_core_and_tool_call_returns_exactly_the_direct_content() {
    let root = folder("compact");
    std::fs::write(root.join("notes.csv"), "a,b\n1,2\n").unwrap();
    let mut m = Mcp::rooted(&root, true);
    let tools = m.rpc("tools/list", json!({}))["result"]["tools"].clone();
    let names: Vec<&str> = tools.as_array().unwrap().iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!((12..=18).contains(&names.len()), "{names:?}");
    for core in [
        "drawing_create",
        "file_attach",
        "open",
        "save",
        "inspect_drawing",
        "query_entities",
        "command_line",
        "execute",
        "list_commands",
        "render",
        "tool_search",
        "tool_call",
    ] {
        assert!(names.contains(&core), "{core} in {names:?}");
    }
    assert!(!names.contains(&"new_drawing"), "compact leaves the rest to tool_search");
    assert!(!names.iter().any(|n| n.contains("list_files") || n.contains("dir")), "no directory listing");
    // Discovery.
    let found = m.ok("tool_search", json!({"query": "new drawing"}));
    assert!(found.to_string().contains("new_drawing"), "{found}");
    let schema = m.ok("tool_search", json!({"name": "new_drawing"}));
    assert_eq!(schema["name"], "new_drawing");
    // tool_call returns exactly what the direct tool returns, embedded files included.
    let direct = m.call("file_attach", json!({"path": "notes.csv"}));
    let meta = m.call("tool_call", json!({"name": "file_attach", "arguments": {"path": "notes.csv"}}));
    assert_eq!(direct, meta);
    assert_eq!(resource(&meta).1, "text/csv");
    assert!(m.err("tool_call", json!({"name": "tool_call", "arguments": {"name": "file_attach"}})).contains("can't run tool_call"));
    assert!(m.err("tool_call", json!({"name": "tool_search", "arguments": {}})).contains("can't run tool_search"));
    assert!(m.err("new_drawing", json!({})).contains("tool_call"), "a tool outside the core runs through tool_call");
    m.ok("tool_call", json!({"name": "new_drawing", "arguments": {"metric": true}}));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn confinement_covers_generic_batch_nested_and_exports() {
    let base = folder("confine");
    let root = base.join("root");
    let outside = base.join("outside");
    std::fs::create_dir_all(&root).unwrap();
    std::fs::create_dir_all(&outside).unwrap();
    let mut maker = Mcp::start(&[]);
    let mut p = plan();
    p["path"] = json!(outside.join("secret.dxf").to_str().unwrap());
    maker.ok("drawing_create", p);
    drop(maker);
    let secret = outside.join("secret.dxf");
    let s = secret.to_str().unwrap().to_string();
    let o = |n: &str| outside.join(n).to_str().unwrap().to_string();

    let mut m = Mcp::rooted(&root, true);
    m.ok("tool_call", json!({"name": "new_drawing", "arguments": {}}));
    m.ok("command_line", json!({"text": "circle 0,0 5"}));
    let refused = |m: &mut Mcp, tool: &str, args: Value| {
        let e = m.call(tool, args.clone()).to_string();
        let cli_refused = e.contains("outside the working folder") || e.contains("path rejected");
        assert!(cli_refused, "{tool} {args}: {e}");
    };
    // Direct tools, absolute and relative escapes.
    refused(&mut m, "open", json!({"path": s}));
    refused(&mut m, "open", json!({"path": "../outside/secret.dxf"}));
    refused(&mut m, "save", json!({"path": o("x.dxf")}));
    refused(&mut m, "save", json!({"path": "../outside/x.pdf"}));
    refused(&mut m, "render", json!({"path": o("x.png")}));
    refused(&mut m, "file_attach", json!({"path": s}));
    refused(&mut m, "drawing_create", json!({"path": o("made.dxf"), "entities": [{"type": "circle", "center": [0, 0], "radius": 1}]}));
    // Generic commands, nested dispatch and command-line input.
    refused(&mut m, "execute", json!({"command": "saveas", "params": {"path": o("y.dxf")}}));
    refused(&mut m, "execute", json!({"command": "insert", "params": {"name": "S", "file": s, "at": [0, 0]}}));
    refused(&mut m, "execute", json!({"command": "import", "params": {"path": s}}));
    refused(&mut m, "execute", json!({"command": "script", "params": {"path": o("x.scr")}}));
    refused(&mut m, "tool_call", json!({"name": "execute", "arguments": {"command": "wblock", "params": {"path": o("w.dxf")}}}));
    refused(&mut m, "tool_call", json!({"name": "open", "arguments": {"path": s}}));
    // A batch: a script typing commands that name paths outside.
    let r = m.call("script", json!({"text": format!("-WBLOCK\n{}\n\n", o("batch.dxf"))})).to_string();
    let r2 = m.call("command_line", json!({"text": format!("_.SAVEAS 2018 {}", o("typed.dxf"))})).to_string();
    let r3 = m.call("execute", json!({"command": "xattach", "params": {"path": s, "at": [0, 0]}})).to_string();
    for r in [&r, &r2, &r3] {
        assert!(!r.contains("\"isError\":false") || r.contains("rejected") || r.contains("outside"), "{r}");
    }
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(&outside, root.join("away")).unwrap();
        std::os::unix::fs::symlink(&secret, root.join("leak.dxf")).unwrap();
        refused(&mut m, "open", json!({"path": "leak.dxf"}));
        refused(&mut m, "file_attach", json!({"path": "leak.dxf"}));
        refused(&mut m, "save", json!({"path": "away/z.dxf"}));
    }
    // Nothing reached the outside folder; only what the tests made is there.
    assert_eq!(names(&outside), ["secret.dxf"], "{r} {r2} {r3}");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn rooted_writes_never_replace_and_default_saves_take_unused_names() {
    let root = folder("noreplace");
    let mut m = Mcp::rooted(&root, false);
    m.ok("drawing_create", plan());
    let first = std::fs::read(root.join("plan.dxf")).unwrap();
    let again = m.ok("drawing_create", plan());
    assert!(again["path"].as_str().unwrap().ends_with("plan (2).dxf"), "{again}");
    m.ok("open", json!({"path": "plan.dxf"}));
    assert!(m.err("save", json!({"path": "plan.dxf"})).contains("never replaced"));
    assert!(m.err("execute", json!({"command": "saveas", "params": {"path": "plan (2).dxf"}})).contains("never replaced"));
    m.ok("save", json!({}));
    m.ok("save", json!({"path": "plan.pdf"}));
    assert!(m.err("save", json!({"path": "plan.pdf"})).contains("never replaced"), "exports too");
    assert_eq!(names(&root), ["plan (2).dxf", "plan (3).dxf", "plan.dxf", "plan.pdf"], "no staging files are left");
    assert_eq!(std::fs::read(root.join("plan.dxf")).unwrap(), first);
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn a_missing_root_and_a_root_with_connect_are_refused() {
    let base = folder("badroot");
    let out = Command::new(BIN).args(["mcp", "--confine", "--root"]).arg(base.join("nope")).stdin(Stdio::null()).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("must be an existing folder"), "{out:?}");
    std::fs::write(base.join("file"), "x").unwrap();
    let out = Command::new(BIN).args(["mcp", "--confine", "--root"]).arg(base.join("file")).stdin(Stdio::null()).output().unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("not a folder"), "{out:?}");
    let out = Command::new(BIN).args(["mcp", "--connect", "127.0.0.1:9", "--root"]).arg(&base).stdin(Stdio::null()).output().unwrap();
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("cannot be used with"), "{out:?}");
    let _ = std::fs::remove_dir_all(base);
}

#[test]
fn hostile_drawing_create_arguments_are_refused_before_anything_is_written() {
    let root = folder("hostile");
    let mut m = Mcp::rooted(&root, false);
    let circle = json!({"type": "circle", "center": [0, 0], "radius": 1});
    let many: Vec<Value> = (0..5001).map(|_| circle.clone()).collect();
    let deep: Vec<Value> = (0..10)
        .map(|i| {
            let mut ents = vec![circle.clone()];
            if i > 0 {
                ents.push(json!({"type": "insert", "name": format!("B{}", i - 1), "at": [0, 0]}));
            }
            json!({"name": format!("B{i}"), "entities": ents})
        })
        .collect();
    let cases = [
        (json!({"path": "a.txt"}), ".dxf or .dwg"),
        (json!({"path": "a.dxf", "entities": many}), "at most 5000"),
        (json!({"path": "a.dxf", "entities": [{"type": "circle", "center": [1e300, 0], "radius": 1}]}), "finite"),
        (json!({"path": "a.dxf", "entities": [{"type": "circle", "center": [0, 0], "radius": -1}]}), "greater than 0"),
        (json!({"path": "a.dxf", "entities": [{"type": "text", "at": [0, 0], "text": "x".repeat(4001)}]}), "1-4000"),
        (json!({"path": "a.dxf", "entities": [{"type": "insert", "name": "Ghost", "at": [0, 0]}]}), "not defined"),
        (json!({"path": "a.dxf", "layers": [{"name": "a/b"}]}), "not a valid name"),
        (json!({"path": "a.dxf", "layers": [{"name": "L", "lineweight": 9}]}), "lineweight"),
        (json!({"path": "a.dxf", "blocks": deep}), "nest more than 8"),
        (json!({"path": "a.dxf", "entities": [{"type": "spline"}]}), "unknown type"),
        (json!({"path": "a.dxf", "entities": [{"type": "line", "points": [[0, 0]]}]}), "2-4096 points"),
        (json!({"path": "a.dxf", "units": "furlong"}), "units"),
        (json!({"path": "a.dxf", "scale": 0}), "scale"),
        (json!({"path": "a.dxf", "entities": [{"type": "hatch", "points": [[0, 0], [1, 0], [1, 1]], "pattern": "../../x"}]}), "pattern"),
    ];
    for (args, want) in cases {
        let e = m.err("drawing_create", args.clone());
        assert!(e.contains(want), "{args}: wanted {want:?} in {e}");
    }
    assert!(names(&root).is_empty(), "nothing written: {:?}", names(&root));
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn file_attach_checks_types_limits_and_encodes_names() {
    let root = folder("attach");
    std::fs::write(root.join("big.pdf"), vec![0u8; 32 * 1024 * 1024 + 1]).unwrap();
    std::fs::write(root.join("Plan Ä #1.svg"), "<svg/>").unwrap();
    std::fs::write(root.join("x.exe"), "MZ").unwrap();
    std::fs::create_dir_all(root.join("folder.pdf")).unwrap();
    let mut m = Mcp::rooted(&root, false);
    assert!(m.err("file_attach", json!({"path": "big.pdf"})).contains("32 MiB"));
    assert!(m.err("file_attach", json!({"path": "x.exe"})).contains("can't attach .exe"));
    assert!(m.err("file_attach", json!({"path": "folder.pdf"})).contains("not a regular file"));
    assert!(m.err("file_attach", json!({"path": "missing.pdf"})).contains("missing.pdf"));
    assert!(m.err("file_attach", json!({"path": ""})).contains("path"));
    let r = m.call("file_attach", json!({"path": "Plan Ä #1.svg"}));
    let (uri, mime, bytes) = resource(&r);
    assert_eq!(uri, "file:///Plan%20%C3%84%20%231.svg");
    assert_eq!(mime, "image/svg+xml");
    assert_eq!(bytes, b"<svg/>");
    let _ = std::fs::remove_dir_all(root);
}

#[test]
fn tools_command_prints_the_catalog_for_deployment_checks() {
    let full: Value = serde_json::from_slice(&Command::new(BIN).args(["tools", "--json"]).output().unwrap().stdout).unwrap();
    let compact: Value = serde_json::from_slice(&Command::new(BIN).args(["tools", "--compact", "--json"]).output().unwrap().stdout).unwrap();
    assert_eq!(full["name"], "cadcraft");
    assert!(full["instructions"].as_str().unwrap().starts_with("CADCraft makes and edits CAD drawings. To make a new drawing, call drawing_create."));
    let n = |v: &Value| v["tools"].as_array().unwrap().len();
    assert!(n(&full) >= 15 && n(&compact) <= 18, "{} {}", n(&full), n(&compact));
    assert!(compact["tools"].to_string().contains("tool_call") && !full["tools"].to_string().contains("tool_call"));
    for t in ["drawing_create", "open"] {
        let d = full["tools"].as_array().unwrap().iter().find(|x| x["name"] == t).unwrap()["description"].to_string();
        assert!(d.contains("Never write Python or other code to create, read or edit a drawing."), "{t}: {d}");
    }
    let text = String::from_utf8(Command::new(BIN).args(["tools"]).output().unwrap().stdout).unwrap();
    assert!(text.contains("drawing_create") && text.contains("MCP tools"), "{text}");
}

/// The Claude Desktop bundle lists every tool the server offers, with the same descriptions
/// (packaging/mcpb/build.sh regenerates the list from `tools --json`).
#[test]
fn the_mcpb_manifest_matches_the_tool_list() {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../packaging/mcpb/manifest.json");
    let manifest: Value = serde_json::from_str(&std::fs::read_to_string(&manifest).unwrap()).unwrap();
    let full: Value = serde_json::from_slice(&Command::new(BIN).args(["tools", "--json"]).output().unwrap().stdout).unwrap();
    let pairs = |tools: &Value| -> Vec<(String, String)> {
        tools.as_array().unwrap().iter().map(|t| (t["name"].as_str().unwrap().into(), t["description"].as_str().unwrap().into())).collect()
    };
    assert_eq!(pairs(&manifest["tools"]), pairs(&full["tools"]), "run packaging/mcpb/build.sh --manifest-only");
    // The bundle may be versioned on its own (MCPB_VERSION), but as a semantic version.
    let v = manifest["version"].as_str().unwrap();
    assert_eq!(v.split('.').filter(|p| p.parse::<u32>().is_ok()).count(), 3, "{v}");
    assert!(manifest["server"]["mcp_config"]["args"].to_string().contains("--read-anywhere"));
}

#[test]
fn create_root_makes_an_absolute_working_folder_and_refuses_a_relative_one() {
    let base = folder("createroot");
    let made = base.join("Documents").join("CADCraft");
    let mut m = Mcp::start(&["--create-root", "--read-anywhere", "--root", made.to_str().unwrap()]);
    m.ok("drawing_create", json!({"path": "a.dxf", "entities": [{"type": "circle", "center": [0, 0], "radius": 1}]}));
    assert_eq!(names(&made), ["a.dxf"]);
    drop(m);
    let out = Command::new(BIN)
        .args(["mcp", "--create-root", "--read-anywhere", "--root", "relative/folder"])
        .current_dir(&base)
        .stdin(Stdio::null())
        .output()
        .unwrap();
    assert!(String::from_utf8_lossy(&out.stderr).contains("must be an absolute path"), "{out:?}");
    assert!(!base.join("relative").exists());
    // An unsubstituted or empty value never runs unconfined.
    let out = Command::new(BIN).args(["mcp", "--create-root", "--read-anywhere", "--root", ""]).stdin(Stdio::null()).output().unwrap();
    assert!(!out.status.success(), "{out:?}");
    let _ = std::fs::remove_dir_all(base);
}

/// Claude Desktop: chat attachments aren't on disk, so the bundle reads from anywhere and finds
/// files by name; every write still lands in the working folder as a new file.
#[test]
fn read_anywhere_opens_files_elsewhere_and_saves_into_the_working_folder() {
    let base = folder("anywhere");
    let (work, elsewhere) = (base.join("work"), base.join("elsewhere/deep"));
    std::fs::create_dir_all(&work).unwrap();
    std::fs::create_dir_all(&elsewhere).unwrap();
    let mut maker = Mcp::start(&[]);
    let mut p = plan();
    p["path"] = json!(elsewhere.join("Dropped Plan.dxf").to_str().unwrap());
    maker.ok("drawing_create", p);
    drop(maker);
    let dropped = elsewhere.join("Dropped Plan.dxf");
    let original = std::fs::read(&dropped).unwrap();

    let mut m = Mcp::start(&["--read-anywhere", "--root", work.to_str().unwrap()]);
    let init = m.rpc("initialize", json!({}));
    assert!(init["result"]["instructions"].as_str().unwrap().contains("find_files"), "{init}");
    let found = m.ok("find_files", json!({"name": "DROPPED plan", "folder": base.to_str().unwrap()}));
    assert_eq!(found["count"], 1, "{found}");
    assert_eq!(Path::new(found["files"][0]["path"].as_str().unwrap()), dropped);
    m.ok("open", json!({"path": dropped.to_str().unwrap()}));
    let saved = m.ok("save", json!({}));
    assert_eq!(Path::new(saved["path"].as_str().unwrap()), work.join("Dropped Plan.dxf"), "{saved}");
    assert!(m.err("save", json!({"path": elsewhere.join("x.dxf").to_str().unwrap()})).contains("outside the working folder"));
    assert!(m.err("render", json!({"path": base.join("x.png").to_str().unwrap()})).contains("outside the working folder"));
    // From bytes, for a small file that can't be reached by path.
    let b64 = {
        const A: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut s = String::new();
        for c in original.chunks(3) {
            let n = c.iter().enumerate().fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
            for i in 0..4 {
                s.push(if i <= c.len() { char::from(A[((n >> (18 - 6 * i)) & 63) as usize]) } else { '=' });
            }
        }
        s
    };
    m.ok("open", json!({"data": b64, "name": "From Chat.dxf"}));
    assert_eq!(m.ok("find_text", json!({"contains": "bedroom"}))["count"], 1);
    let saved = m.ok("save", json!({}));
    assert_eq!(Path::new(saved["path"].as_str().unwrap()), work.join("From Chat.dxf"));
    assert!(m.err("open", json!({"data": "A".repeat(3 * 1024 * 1024), "name": "big.dxf"})).contains("2 MiB"));
    assert!(m.err("open", json!({})).contains("`path` or `data`"));
    assert_eq!(std::fs::read(&dropped).unwrap(), original, "the original is untouched");
    assert_eq!(names(&work), ["Dropped Plan.dxf", "From Chat.dxf"]);
    drop(m);

    // Without --read-anywhere the working folder still confines reads, and compact (LibreChat)
    // never offers find_files.
    let mut strict = Mcp::rooted(&work, false);
    assert!(strict.err("open", json!({"path": dropped.to_str().unwrap()})).contains("outside the working folder"));
    let mut compact = Mcp::rooted(&work, true);
    let tools = compact.rpc("tools/list", json!({}))["result"]["tools"].clone();
    assert!(tools.as_array().unwrap().iter().all(|t| t["name"] != "find_files"), "{tools}");
    assert!(compact.err("tool_call", json!({"name": "find_files", "arguments": {"name": "x"}})).contains("compact"));
    assert!(compact.ok("tool_search", json!({"query": "find"})).as_array().unwrap().iter().all(|t| t["name"] != "find_files"));
    let _ = std::fs::remove_dir_all(base);
}

/// ChatGPT plugin: `--root ~/Documents/CADCraft` alone is unrestricted. The folder is made under
/// the home folder and new drawings go there, files elsewhere are opened and saved where they are,
/// and dropped files are found by name.
#[test]
fn an_unconfined_root_is_a_home_relative_working_folder_and_reaches_everything() {
    let base = folder("unconfined");
    let home = base.join("home");
    let elsewhere = base.join("elsewhere");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let mut child = Command::new(BIN)
        .args(["mcp", "--root", "~/Documents/CADCraft"])
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("CADCRAFT_CONFIG_DIR", "")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let (stdin, stdout) = (child.stdin.take().unwrap(), BufReader::new(child.stdout.take().unwrap()));
    let mut m = Mcp { child, stdin, stdout, id: 0 };
    let work = home.join("Documents").join("CADCraft");
    let init = m.rpc("initialize", json!({}));
    let text = init["result"]["instructions"].as_str().unwrap();
    assert!(text.contains(&work.display().to_string()) && text.contains("find_files"), "{text}");
    assert!(!text.contains("never replaces"), "{text}");
    m.ok("drawing_create", plan());
    assert_eq!(names(&work), ["plan.dxf"]);
    // Anywhere, in place: an explicit path outside the folder, then a save without a path.
    let other = elsewhere.join("other.dxf");
    m.ok("open", json!({"path": "plan.dxf"}));
    m.ok("save", json!({"path": other.to_str().unwrap()}));
    m.ok("open", json!({"path": other.to_str().unwrap()}));
    m.ok("command_line", json!({"text": "circle 0,0 1"}));
    let saved = m.ok("save", json!({}));
    assert_eq!(Path::new(saved["path"].as_str().unwrap()), other, "{saved}");
    let found = m.ok("find_files", json!({"name": "other", "folder": base.to_str().unwrap()}));
    assert!(found["files"].as_array().unwrap().iter().any(|f| Path::new(f["path"].as_str().unwrap()) == other), "{found}");
    assert!(m.call("file_attach", json!({"path": other.to_str().unwrap()}))["isError"] != json!(true));
    drop(m);
    let _ = std::fs::remove_dir_all(base);
}
