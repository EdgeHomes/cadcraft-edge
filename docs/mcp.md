# CADCraft and AI agents (MCP)

CADCraft speaks the [Model Context Protocol](https://modelcontextprotocol.io) over stdio.

## Start it

```sh
# Headless: an in-process drawing session, no window.
cadcraft-cli mcp

# Bridged to the running app (watch the agent draw):
cadcraft --control 7979 &
cadcraft-cli mcp --connect 127.0.0.1:7979
```

An MCP client usually starts its servers when it starts, often before the app is open. With
`--lazy` the bridge starts anyway and reaches the app at the first tool call; until the app answers,
each call fails with a message saying how to open it. `--launch` also opens CADCraft (macOS) with
that control port at a call that finds nothing, and waits for it:

```json
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp", "--connect", "127.0.0.1:7979", "--launch"] } } }
```

`--connect` needs the app's `HOST:PORT`. Without one (or with an unknown argument) `cadcraft-cli mcp`
exits with an error instead of falling back to a headless session, so a client meant for the app
never edits a separate drawing by mistake. If the app can't be reached, startup fails too.

Example client configuration (Claude Code / Claude Desktop):

```json
{ "mcpServers": { "cadcraft": { "command": "cadcraft-cli", "args": ["mcp"] } } }
```

## Tools

| Tool | What it does |
|---|---|
| `command_line {text}` | Type at the command line exactly like a person: `line 0,0 @10,0 @0,5 c`, `circle 5,2 1`, `offset 0.5`, `zoom e`. Spaces act as Enter; empty text is Enter. Returns the new prompt, keywords and output. |
| `script {text}` | A multi-line script (like `.scr` files). |
| `execute {command, params}` | Any command with JSON parameters, never a dialog. `list_commands` shows ids and parameter docs. |
| `list_commands {filter?}` | The command catalog; `filter` matches part of a command's id, label or an alias. |
| `inspect_drawing {entities?, limit?}` | Counts, extents, layers, styles, blocks, layouts, selection, undo history and entities. |
| `query_entities {type?, layer?, window?, crossing?, limit?, offset?}` | Filtered entities with handles and geometry. `window` `[[x0,y0],[x1,y1]]` finds objects whose geometry is inside it or crosses it (`crossing: false`: entirely inside). `count` is every match, `returned` the page (`offset`, `limit`, default 500). |
| `execute {"command": "shapes", "params": {type?, layer?, window?, crossing?, limit?, offset?, tolerance?}}` | Facts about the closed shapes — closed polylines, circles, full ellipses, closed splines: perimeter, area, centroid, winding, the first self-intersection and coincident vertices, exact with arcs and bulges. Other matches are counted under `skipped` with the reason. `selfCheck` says what was found, analysed and skipped, the caps hit with their values (`limit`, `crossingSegments`) and implausible values (perimeter past 10× the bounding-box diagonal, area larger than the drawing extents, each past a 1e-9 relative tolerance for rounding; with no visible objects there are no extents, so no area bound applies). |
| `execute {"command": "shapes", "params": {type?, layer?, window?, crossing?, limit?, offset?, tolerance?}}` | Facts about the closed shapes — closed polylines, circles, full ellipses, closed splines: perimeter, area, centroid, winding, the first self-intersection and coincident vertices, exact with arcs and bulges. Other matches are counted under `skipped` with the reason. |
| `execute {"command": "usage", "params": {type?, limit?, offset?}}` | Read-only cross-reference of the layers, blocks, text styles and linetypes: for each, the objects in model space and layouts that use it, the objects in block definitions that use it, whether it is used only inside block definitions, and whether it is empty or unused. `summary` counts each kind and `unused` lists what PURGE would offer, in table order. One scan reads at most 5,000,000 objects; a larger drawing sets `incomplete: true` with an `incompleteReason`, because counts, statuses and summaries then omit the objects past the limit. |
| `execute {"command": "loops", "params": {tolerance?, maxPairs?, limit?, offset?}}` | Implied closed loops from loose LINE and ARC chains: each closed loop with its member handles, perimeter, area and winding, exact with arcs; each open chain with the endpoint gap, its location (`at`, `ends`) and length; and branch junctions (three or more ends meeting), reported but not resolved. `tolerance` defaults to 1e-9 × the extents diagonal; the quadratic endpoint pairing stops at `maxPairs` (default 1,000,000) and reports `capped` with `pairTestsNeeded` instead of guessing from a partial pairing. |
| `query_entities {type?, layer?, selected?, window?, crossing?, limit?, offset?}` | Filtered entities with handles and geometry. `selected: true` restricts results to the current selection in the active space; an empty selection returns no entities. Omitted/false queries all entities in that space. Filters combine before paging. `window` `[[x0,y0],[x1,y1]]` finds objects whose geometry is inside it or crosses it (`crossing: false`: entirely inside). `count` is every match, `returned` the page (`offset`, `limit`, default 500). |
| `render {width?, height?, fit?, window?, background?, plot_style?}` | A PNG image of the drawing, or of just the area `window` `[[x0,y0],[x1,y1]]` (one sheet or room of a large drawing; 1600 px wide with the window's shape unless sized). `background`: `"dark"` (the canvas; the default without a plot style), `"white"` (the default with one, as a plot prints on paper), `"paper"` or `"#rrggbb"`; colour 7 is drawn white on a dark background and black on a light one, as on a plot. `plot_style`: a plot style table, e.g. `monochrome.ctb` (every colour black) or `grayscale.ctb`; `"none"` (or `""`) draws every object with its own properties, also in a layout that shows its own plot styles (without `plot_style` such a layout draws with its table, as on screen). An unknown value is refused. The result is an image content block (`content[0].type` is `"image"`), not text. |
| `find_text {contains?, layer?, window?, limit?, offset?}` | The readable texts — TEXT, MTEXT without formatting codes, block attributes, multileaders — with text, insertion point, height, rotation, style, layer and handle; filtered by content (any case), layer and a window around the insertion points. Read-only (the `texts` command). |
| `new_drawing {metric?}`, `open {path}`, `save {path?}`, `cancel` | Files and Escape. `save` to a `.svg`, `.png` or `.pdf` path exports: the drawing keeps its name and unsaved changes. |
| `drawing_create {path, units?, style?, scale?, version?, layers?, blocks?, entities}` | A new, editable drawing in one call, built only from CADCraft commands: layers (name, colour, linetype, lineweight), blocks and inserts, lines, polylines, rectangles, circles, arcs, text, mtext, dimensions (linear, aligned, radius, diameter) and hatches. Text, dimensions and hatch spacing follow a plot scale chosen to fit an A3 sheet (2.5 mm text; `scale` overrides). Validated and capped first (5000 entities, 100 blocks nested at most 8 deep, 256 layers, 4096 points per entity, 4000 characters per text and 200,000 in all, coordinates finite within ±1e9, 32 MiB output); invalid input writes nothing, and an entity the engine refuses is reported in `dropped`. Saved atomically under an unused name; returns a JSON summary and the file as an embedded resource. |
| `file_attach {path}` | A file a tool wrote, as a download: `{"type":"resource","resource":{"uri":"file:///<percent-encoded name>","mimeType":…,"blob":<base64>}}`. Regular files only, up to 32 MiB (the read itself is bounded): dwg (`application/acad`), dxf (`application/dxf`), pdf, svg, png, jpeg, csv, txt, docx, xlsx, pptx. |
| `screenshot`, `ui_inspect`, `ui_click {x, y}` | Only when connected to the app. |

Resources: `cadcraft://drawing` (inspect JSON) and `cadcraft://commands` (catalog).

Arguments are checked against each tool's input schema. An unknown tool, `arguments` that aren't an
object, a missing required argument or one of the wrong type is a JSON-RPC error `-32602` (invalid
params), so `command_line {}` never presses Enter by accident. A tool that runs and fails (a bad
point, an unknown command) returns a result with `isError: true` and the message.

## Chat deployments: `--root`, `--compact` and `tools`

```sh
cadcraft-cli mcp --root ~/Documents/CADCraft   # ChatGPT and Claude Desktop (packaging/chatgpt, packaging/mcpb): unrestricted
cadcraft-cli mcp --compact --confine --root /app/uploads/cadcraft   # LibreChat (npm: cadcraft-edge-mcp)
cadcraft-cli tools [--compact] [--json]   # the tool list and instructions, for checking a deployment
```

- `--root DIR` (or `--root=DIR`) alone names the working folder and restricts nothing: the server
  starts in it, so relative paths and new drawings go there, `~` is the home folder (`HOME`, else
  `USERPROFILE`), and a missing folder is made. Files anywhere else are opened and saved as without
  `--root`, and `find_files` searches the usual folders.
- `--confine` (with `--root`) confines every file any tool reads or writes to `DIR`, which must
  exist (`--create-root` makes it; it must then be absolute). The npm launcher always adds it
  unless `--read-anywhere` is given: a shared server must never run unconfined. Relative paths are inside it;
  absolute paths inside it are accepted; `..` and links that lead out are refused, and so are
  Windows device names. It covers every path: the file tools, `execute` and `command_line`
  (OPEN, SAVEAS, WBLOCK, INSERT from a file, IMPORT, XATTACH, IMAGEATTACH, SCRIPT, plot and export,
  table and data-extraction exports, plot style and linetype files, LISP `open`), xrefs and images
  a drawing refers to, and `tool_call`. The engine's file gateways (`crates/engine/src/file_access.rs`)
  enforce it for the whole process, and a test keeps commands from touching the file system any
  other way. The settings folder (plotter configurations) is turned off. `--root` can't be combined
  with `--connect`: a running app's files can't be confined.
- Under `--confine` or `--read-anywhere` nothing is replaced: every write stages the bytes beside the target and publishes
  them with a hard link, which fails if the name is taken (the folder's file system must support
  hard links). `save` without a path writes an unused name beside the opened drawing
  (`plan (2).dxf`); no `.bak` is written.
- `--read-anywhere` (with `--root`) confines writes like `--confine`
  but lets tools read files anywhere. A save without a path of a drawing opened from outside
  the root, or from bytes, lands in the root under its name. `find_files {name, folder?, limit?}`
  is in the full list (never in `--compact`, not even through `tool_call`); it searches beyond the
  working folder only when reads aren't confined:
  files whose name contains `name`, newest first, searched in the working folder, Desktop,
  Downloads, Documents and home (hidden, `Library`, `AppData` and `node_modules` folders skipped;
  200,000 entries, depth 12 and 10 s at most; links not followed). `open {data, name}` opens a file
  sent as base64, up to 2 MiB.
- `--compact` lists 12 core tools (`drawing_create`, `file_attach`, `open`, `save`,
  `inspect_drawing`, `query_entities`, `find_text`, `command_line`, `script`, `execute`,
  `list_commands`, `render`) plus `tool_search` and `tool_call`. `tool_call` returns exactly what
  the direct tool returns, embedded files included, and refuses to run the meta tools. There is no
  directory-listing tool.

The server instructions tell the model to use `drawing_create` for new drawings and to open an
attached upload's exact absolute path, edit it, save under a new name and `file_attach` the result,
never writing code to handle drawings. `open` reports `warnings` (what reading left out).

**Formats.** DXF is CADCraft's native format and the recommended output; DWG is written through a
DXF-to-DWG conversion (`docs/file-format-parity.md`), so it keeps what DXF keeps, minus the bridge's
gaps. Neither round trip is lossless for content CADCraft doesn't model (proxy objects, some
3D/ACIS data, unknown XDATA in DWG); `open` lists what was left out. **MIME types:** IANA's
`image/vnd.dwg` and `image/vnd.dxf` are avoided because chat clients show `image/*` as pictures.

## A typical session

1. `command_line {"text": "rectang 0,0 100,60"}`
2. `execute {"command": "circle", "params": {"center": [10, 10], "radius": 4}}`
3. `command_line {"text": "dimlinear 0,0 100,0 50,-8"}`
4. `inspect_drawing {}` — verify counts and geometry.
5. `render {}` — look at it.

The end-to-end test in `crates/mcp/src/tests.rs` runs a session like this and checks the result
only through MCP.
