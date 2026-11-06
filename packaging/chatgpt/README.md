# CADCraft for ChatGPT Desktop (plugin)

`cadcraft-chatgpt-<version>.zip` is a ChatGPT plugin that runs CADCraft's own MCP server
(`cadcraft-cli mcp`) on this computer, on macOS (universal) and Windows (x86_64). It offers every
tool: `drawing_create`, `open`, `find_files`, `save`, `file_attach`, `inspect_drawing`,
`query_entities`, `find_text`, `command_line`, `script`, `execute`, `list_commands`, `render`,
`new_drawing` and `cancel`.

## Install

In the ChatGPT workspace settings: **Add → Upload plugin**, choose the zip. It is a
desktop-only plugin (a local server), so it is used from the ChatGPT desktop app.

## Files

The server starts as `cadcraft-cli mcp --root ~/Documents/CADCraft`:

- New drawings and relative paths go to `Documents/CADCraft`, which is made on first start. On
  Windows `~/Documents` is the Documents folder Windows reports (OneDrive included), asked of
  PowerShell at start.
- Nothing else is restricted: drawings anywhere on the computer can be opened and saved. A file
  dropped into a chat isn't on this computer's disk, so the model finds the original by name with
  `find_files`; a small one it can't find (up to 2 MiB) can be sent as bytes.

## Build

```sh
packaging/chatgpt/build.sh
```

Builds the same binaries as `packaging/mcpb/build.sh` (lipo for macOS, `cargo xwin` for Windows),
stages `plugin/` with them, the icon and the licences, and writes `dist/cadcraft-chatgpt-<version>.zip`
(`PLUGIN_VERSION` versions it on its own). The macOS binary is ad-hoc signed only; see
`packaging/mcpb/README.md` for signing and notarization.
