# cadcraft-edge-mcp

CADCraft's native Rust CAD engine as a stdio MCP server. The small Node launcher picks the
prebuilt `cadcraft-cli` from its platform optional dependency (`cadcraft-edge-mcp-<os>-<cpu>`, or
`$CADCRAFT_CLI`) and runs `cadcraft-cli mcp`, adding `--root $CADCRAFT_ROOT` (else the working
directory) unless you pass `--root DIR` or `--root=DIR` yourself, and always `--confine` (unless you pass
`--read-anywhere`), so a shared server never runs unconfined. Stdio, signals and the exit code
pass straight through. All drawing work, the protocol and file confinement run in Rust.

Platforms: macOS arm64, Linux x64 and Linux arm64. The Linux binaries are static musl builds, so
they run on Alpine (LibreChat's image) without a display or audio device.

**If npm skipped the platform package** (npm silently skips an optional dependency it can't
fetch, and `npx` then keeps that incomplete install in its cache), the launcher installs
`cadcraft-edge-mcp-<os>-<cpu>` at its own exact version once into
`~/.cache/cadcraft-edge-mcp/<version>` (`$CADCRAFT_EDGE_MCP_CACHE` moves it), logging to stderr.
If that fails too, it exits with a message naming the `_npx` cache folder to delete.

## LibreChat

Under `mcpServers` in `librechat.yaml`:

```yaml
cadcraft:
  type: stdio
  command: npx
  args: ["-y", "cadcraft-edge-mcp", "--compact", "--root", "/app/uploads/cadcraft"]
  fileInputDir: /app/uploads/cadcraft
  serverInstructions: true
  timeout: 120000
  initTimeout: 120000
```

**`/app/uploads/cadcraft` must exist before the server connects**: the server stops otherwise.

One root shared by everyone is fine for development; production needs a root per user, or every
chat can read every other chat's uploads.

## Make a drawing

`drawing_create` builds an editable `.dxf` (or `.dwg`) in one call and returns it for download:

```json
{"path": "plan.dxf", "units": "mm",
 "layers": [{"name": "Walls", "color": 7, "lineweight": 0.5}],
 "entities": [
   {"type": "rectangle", "corner1": [0, 0], "corner2": [10000, 6000], "layer": "Walls"},
   {"type": "text", "at": [5000, 3000], "text": "LIVING ROOM", "justify": "MC"},
   {"type": "dimension", "p1": [0, 0], "p2": [10000, 0], "at": [5000, -1000]}
 ]}
```

## Edit an upload

1. `open` with the exact absolute upload path; tell the user about any `warnings`.
2. `inspect_drawing`, `query_entities`, `find_text`, `render`; edit with `command_line` (AutoCAD
   syntax: `-layer`, `move`, `rotate`, `scale`, `offset`, `trim`…) or `execute` (`list_commands`
   finds ids and parameters).
3. `save` (no path: `Name (2).dxf` beside the upload; nothing is ever overwritten).
4. `file_attach` with the returned path.

## Check a deployment

```sh
"$(dirname "$(node -p "require.resolve('cadcraft-edge-mcp-linux-x64/package.json')")")/bin/cadcraft-cli" tools --compact
```

## Publish (maintainers)

`packaging/npm/stage.sh` builds and stages every package in `packaging/npm/dist`. Publish the
platform packages before the launcher:

```sh
cd packaging/npm/dist && for d in cadcraft-edge-mcp-*/ cadcraft-edge-mcp/; do npm publish "$d" --access public || break; done
```
