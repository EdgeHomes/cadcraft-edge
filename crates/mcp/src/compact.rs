//! `--compact`: a short core tool list for chat clients, with every other tool reachable through
//! `tool_search` (find it, read its schema) and `tool_call` (run it).

use serde_json::{Value, json};

use crate::tools::tool_definitions;

/// The core tools: enough to create a drawing, and to open, inspect, edit, save, look at and
/// attach an uploaded one, without `tool_search`. Layers, views and editing (move, copy, rotate,
/// scale, offset, trim, text, properties, blocks) go through `command_line`, `script` and
/// `execute`, which reach every CADCraft command.
pub const CORE: &[&str] = &[
    "drawing_create",
    "file_attach",
    "open",
    "save",
    "inspect_drawing",
    "query_entities",
    "find_text",
    "command_line",
    "script",
    "execute",
    "list_commands",
    "render",
];

/// The meta tools, which `tool_call` refuses to run.
pub const META: &[&str] = &["tool_search", "tool_call"];

/// The compact `tools/list`: the core tools, then the meta tools.
pub fn definitions() -> Vec<Value> {
    let mut tools: Vec<Value> = all().into_iter().filter(|t| CORE.iter().any(|n| t["name"] == *n)).collect();
    tools.push(json!({
        "name": "tool_search",
        "description": "Find more CADCraft tools (new_drawing, cancel…) by words in their name or description, or get one tool's full schema by exact `name`. Run what you find with tool_call. CAD commands themselves (LAYER, MOVE, TRIM…) are found with list_commands and run with execute or command_line.",
        "inputSchema": {"type": "object", "properties": {
            "query": {"type": "string", "description": "Words to look for"},
            "name": {"type": "string", "description": "Exact tool name: returns its full definition"}
        }}
    }));
    tools.push(json!({
        "name": "tool_call",
        "description": "Run any CADCraft tool by name with its arguments. Returns exactly what calling the tool directly returns, attached files included. It can't run tool_search or tool_call.",
        "inputSchema": {"type": "object", "required": ["name"], "properties": {
            "name": {"type": "string", "description": "Tool name"},
            "arguments": {"type": "object", "description": "The tool's arguments"}
        }}
    }));
    tools
}

/// Every direct tool of a headless server (what `tool_call` can run).
fn all() -> Vec<Value> {
    tool_definitions(false)["tools"].as_array().cloned().unwrap_or_default()
}

/// `tool_search {query?, name?}`.
pub fn search(args: &Value) -> Result<Value, String> {
    let empty = serde_json::Map::new();
    let obj = match args {
        Value::Null => &empty,
        Value::Object(o) => o,
        _ => return Err("tool_search needs an object".into()),
    };
    if let Some((k, _)) = obj.iter().find(|(k, v)| !matches!(k.as_str(), "query" | "name") || !v.is_string()) {
        return Err(format!("tool_search: unexpected argument {k:?} (expected string `query` or `name`)"));
    }
    let tools = all();
    if let Some(name) = obj.get("name").and_then(Value::as_str) {
        return tools.into_iter().find(|t| t["name"] == name).ok_or_else(|| format!("tool_search: no tool `{name}`"));
    }
    let words: Vec<String> =
        obj.get("query").and_then(Value::as_str).unwrap_or_default().to_lowercase().split_whitespace().map(String::from).collect();
    Ok(Value::Array(
        tools
            .into_iter()
            .filter(|t| {
                let text = format!("{} {}", t["name"].as_str().unwrap_or_default(), t["description"].as_str().unwrap_or_default()).to_lowercase();
                words.iter().all(|w| text.contains(w.as_str()))
            })
            .map(|t| json!({"name": t["name"], "description": t["description"]}))
            .collect(),
    ))
}
