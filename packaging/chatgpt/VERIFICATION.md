# ChatGPT plugin verification

## 0.17.2 (2026-11-06)

`PLUGIN_VERSION=0.17.2 packaging/chatgpt/build.sh` → `dist/cadcraft-chatgpt-0.17.2.zip`, **42.0 MB**
(41,997,214 bytes; 108.6 MB unpacked, 11 files; SHA-256
`b56a5beaf6076d1c15844677a280cc04082a938ffb8552c07c65d651f13cd532`). The binaries report 0.17.0
(the workspace version).

| File | Check |
|---|---|
| `server/cadcraft-cli` | `file`: Mach-O universal, x86_64 + arm64; mode 755 in the zip |
| `server/cadcraft-cli.exe` | `file`: PE32+ console x86-64; mode 755 in the zip |
| `plugin.json`, `mcp.json` | Agent Plugins 1.0 manifest and stdio server, `cwd` `${PLUGIN_ROOT}` |
| `.codex-plugin/plugin.json`, `.mcp.json` | Codex layout as ChatGPT's own bundled plugins use it: `command` `./server/…`, `cwd` `.` |
| `assets/icon.png` | CADCraft's own icon (ATTRIBUTION.md), 512 × 512 |

The layout follows OpenAI's plugin packaging page and ChatGPT's own bundled plugins with local
servers (`./bin/<launcher>` with `cwd: "."`). The zip has the plugin at its root, under 100 MB.

Checked on this Mac (arm64) with the zip **unpacked**, the server started from `.mcp.json` exactly
as configured (cwd the plugin folder, `HOME` a fresh folder): 15 tools; `Documents/CADCraft` made
under the home folder and named in the instructions; `drawing_create` wrote `Two Rooms.dxf` there;
a drawing in `Downloads` was found by name with `find_files`, opened, edited, saved beside it and
attached (`application/dxf`); `render` returned an image.

Test: `an_unconfined_root_is_a_home_relative_working_folder_and_reaches_everything`, seen to fail
with `--root` confining again and `~` not expanded, and to pass restored.

**Not checked:** uploading and running inside ChatGPT Desktop (macOS or Windows), the Windows
binary at runtime (no Windows machine; Windows adding `.exe` to `./server/cadcraft-cli` is
expected of the launcher, not seen), the x86_64 slice, and Gatekeeper on the unpacked binary (it
is ad-hoc signed, not notarized; see `../mcpb/README.md`).
