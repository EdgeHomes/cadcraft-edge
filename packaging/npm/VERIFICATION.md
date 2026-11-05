# npm MCP (LibreChat) and shared server verification

Date: 2026-11-05. Staged version: **0.17.0**. Toolchain: **Rust 1.97.1**; cargo-zigbuild for the
static musl builds; Node 26.3 / npm 11.16 on macOS, `node:22-alpine` (Node 22.23) in Docker. No
fonts embedded. This records local verification only: **nothing was published** and LibreChat
was not run. The Claude Desktop bundle's own checks are in
[`../mcpb/VERIFICATION.md`](../mcpb/VERIFICATION.md).

## Automated checks

Scoped to the changed crates, each under a timeout (the full GUI workspace build of
`cargo xtask ci` was not run):

```sh
RUSTUP_TOOLCHAIN=1.97.1 cargo fmt --all -- --check
RUSTUP_TOOLCHAIN=1.97.1 cargo clippy -p cadcraft-engine -p cadcraft-mcp -p cadcraft-cli --all-targets -- -D warnings
RUSTUP_TOOLCHAIN=1.97.1 cargo test --no-fail-fast -p cadcraft-engine -p cadcraft-mcp -p cadcraft-cli
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask wasm      # passes; its 3 dead-code warnings are in ui-egui, untouched here
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask layers    # 16 crates, no violations
RUSTUP_TOOLCHAIN=1.97.1 cargo xtask assets    # 28 asset files attributed; no asset was added
RUSTUP_TOOLCHAIN=1.97.1 cargo xwin test --no-run -p cadcraft-engine --lib --target x86_64-pc-windows-msvc
```

Format, clippy (warnings denied), wasm, layers and assets pass. Tests: **1,652 passed, 2 failed**
(engine 1,543, MCP 38, CLI 73 listed). The two failures are `non_unicode_output_names` and
`read_file_refuses_a_name_that_is_not_unicode`: APFS refuses to create a file whose name isn't
UTF-8 (`EILSEQ`, os error 92). They fail the same way on the unchanged `main` and have nothing to do
with this change. The engine's tests, including the Windows-only path test, compile for
`x86_64-pc-windows-msvc`; they were not run on Windows.

The headless dependency tree of `cadcraft-cli` for `x86_64-unknown-linux-musl` was audited first:
no windowing, GPU, audio, D-Bus, OpenSSL, egui or file-dialog crates, so no feature boundary was
needed and the desktop app and wasm builds are untouched.

New tests: `file_access::tests` (engine, 7: inside/outside, `..`, links, missing root, device
names, thread scopes and new-only writes, and a scan that fails if any engine code outside the
gateways touches `std::fs`), `delivery`/`create` unit tests (MCP), and
`apps/cadcraft-cli/tests/mcp_chat.rs` (11, the real binary over stdio).

## Tests fail when their fixes are reverted

Each row: the production code patched back, the named test run and seen to fail at runtime (a
panic or assertion, never a compile error), the source restored and the test passing again. Each
revert removes the whole behaviour, not one clause of it: the size limit lost both its metadata
check and its bounded read; confinement was removed by making no root ever be in force, which
also turns off new-only writes; the hard-link revert replaced the whole no-replace branch with the
replacing rename, with no earlier existence check that could keep the test passing.

| Test | Reverted behaviour |
|---|---|
| `confinement_covers_generic_batch_nested_and_exports` | No root ever in force (`check_read`, `check_write`, `replace_allowed`) |
| `rooted_writes_never_replace_and_default_saves_take_unused_names` | Hard-link publication replaced by the replacing rename |
| `absolute_upload_edit_save_as_attach_keeps_the_original_and_untouched_entities` | A save without a path wrote to the opened file |
| `compact_lists_the_core_and_tool_call_returns_exactly_the_direct_content` | Listed every tool regardless of `--compact` |
| `compact_lists_the_core_and_tool_call_returns_exactly_the_direct_content` | Removed the recursive meta-call refusal |
| `hostile_drawing_create_arguments_are_refused_before_anything_is_written` | Removed the entity cap |
| `file_attach_checks_types_limits_and_encodes_names` | Removed the 32 MiB limit (metadata check and bounded read) |
| `file_attach_checks_types_limits_and_encodes_names` | Removed percent-encoding of resource names |
| `drawing_create_makes_a_valid_file_that_reopens_and_attaches` | DXF MIME type back to IANA `image/vnd.dxf` |
| `drawing_create_makes_a_valid_file_that_reopens_and_attaches` | Removed the automatic plot scale (always 1:1) |
| `drawing_create_makes_a_valid_file_that_reopens_and_attaches` | Server did not move into `--root` (relative paths not the root's) |
| `a_missing_root_and_a_root_with_connect_are_refused` | Allowed `--root` with `--connect` |
| `create_root_makes_an_absolute_working_folder_and_refuses_a_relative_one` | `--create-root` accepted a relative folder |
| `tools_command_prints_the_catalog_for_deployment_checks` | Removed the `tools` subcommand's dispatch |
| `file_access::tests::commands_do_not_touch_the_file_system_themselves` | A command read its file with `std::fs::read` |
| `the_mcpb_manifest_matches_the_tool_list` | The bundle manifest drifted (a tool renamed) |

## Design review

Rendered with CADCraft (`render`, white background, `monochrome.ctb`) and looked at:

- A 10 × 6 m metric floor plan (1:50 chosen automatically): walls, door swing, a block inserted
  twice (once rotated 45°), a hatched strip, room names, a two-line note, horizontal, vertical and
  radius dimensions. Zoomed renders confirmed dimension text clears its line.
- The same plan after the upload edit (`MAIN BEDROOM`).
- A Ø120 mm flange at 1:1: centre lines in the `CENTER` linetype, four bolt holes, diameter
  dimensions, a hatched section, a SOLID fill.
- Cabin plans in feet and in inches with `style: architectural`: tick marks; inch drawings read
  20', 32'.

Found and fixed in review:

- Inch drawings with the architectural style dimensioned in decimals; they now use feet and inches
  (`DIMLUNIT 4`). Feet drawings keep decimals (feet-and-inch format needs inch units).
- A title placed by the caller overlapped a dimension's text. `drawing_create` doesn't move
  annotation the caller placed, so the instructions now tell the model to render the result and
  move anything that overlaps.

No clipped text, wrong scales, broken dimensions or broken hatches remain in these drawings.

## Distribution and runtime

`RUSTUP_TOOLCHAIN=1.97.1 packaging/npm/stage.sh` staged, then `npm pack`:

| Package | Target | Binary | Tarball | Runtime check |
|---|---|---|---|---|
| `cadcraft-edge-mcp-darwin-arm64` | `aarch64-apple-darwin` | Mach-O arm64, 34.0 MB | 13.5 MB | Native macOS; fresh offline `npm install` of the tarballs, run through the launcher |
| `cadcraft-edge-mcp-linux-x64` | `x86_64-unknown-linux-musl` | static ELF (`file`: statically linked), 32.6 MB | 13.8 MB | Bare `alpine:3.22` `linux/amd64`, `--network none`; `node:22-alpine` `linux/amd64` offline install through the launcher |
| `cadcraft-edge-mcp-linux-arm64` | `aarch64-unknown-linux-musl` | static ELF (`file`: statically linked), 27.2 MB | 12.7 MB | Bare `alpine:3.22` `linux/arm64`, `--network none`; `node:22-alpine` `linux/arm64` offline install through the launcher |
| `cadcraft-edge-mcp` | Node launcher | — | 7.5 KB | Exact `0.17.0` optional dependencies on all three |

Platform packages contain only `bin/cadcraft-cli`, `package.json` and `licenses/` (CADCraft's
`LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`). No fonts are embedded, so no font licences are
shipped. The size is all engine code (DWG bridge, renderer, PDF backends). The containers had no
`/dev/snd`, no `/dev/dri` and no `DISPLAY`. amd64 containers ran under Docker Desktop's emulation
on an arm64 Mac.

Every runtime row passed the same stdio script with `--compact --root` (request files on stdin,
replies checked on the host, so nothing is needed where the binary runs):

1. `initialize`: the instructions begin "CADCraft makes and edits CAD drawings. To make a new
   drawing, call drawing_create."; 14 tools including `tool_search` and `tool_call`.
2. One `drawing_create` call (`Site Plan.dxf`, 1:50, nothing dropped). The resource is
   `file:///Site%20Plan.dxf`, `application/dxf`; `file_attach` returns the same bytes; `render`
   returns a PNG.
3. The file is copied to `inputs/5d0c9a1e/Site Plan.dxf` (LibreChat's `fileInputDir` layout). A new
   server opens that exact absolute path, edits `BEDROOM` with `textedit`, adds a layer with
   `-layer`, saves with no path (`Site Plan (2).dxf`), and `tool_call` → `file_attach` returns it
   as `file:///Site%20Plan%20%282%29.dxf`. `file_attach /etc/hostname` is refused. The upload's
   SHA-256 is unchanged.
4. A third server reopens the edit: the text is changed and both texts, the dimension, the hatch,
   the new layer and the `Chair` block are there.

Every platform produced identical files: created
`b8a3583caed825243d96184c5e7c5a5de88e6bc32ed1aa93b7e61f307a6fb690`, edited
`89ba397c7bdc6e8c121d57bdd241aa039f887c2d334efc4ab7a49770add44b3d`.

A missing `/app/uploads/cadcraft` fails clearly with exit code 1, both bare and through the
launcher in `node:22-alpine`.

Launcher checks (macOS):

- `CADCRAFT_CLI` overrides the binary; `CADCRAFT_ROOT`, else the working directory, becomes the
  root; an explicit `--root DIR` or `--root=DIR` takes precedence; `mcp` is inserted before the
  arguments.
- The child's exit code 37 passes through; SIGTERM is forwarded (the child's trap exits 23 and the
  launcher returns 23).
- **npx optional-dependency trap**, with the launcher placed in an npx-style
  `_npx/4f2a9c1e/node_modules` and no platform package:
  - offline (`npm_config_offline=true`): it tries the one-time install, fails, and exits 1 naming
    `…/_npx/4f2a9c1e` to delete, plus `npm install cadcraft-edge-mcp-darwin-arm64@0.17.0` and
    `CADCRAFT_CLI` as alternatives;
  - with a registry (a local static registry serving the packed platform tarball): it installs
    `cadcraft-edge-mcp-darwin-arm64@0.17.0` once into `$CADCRAFT_EDGE_MCP_CACHE/0.17.0` with npm's
    output on stderr, then answers `initialize` on a clean stdout. That install took about a
    minute against the throwaway local registry, within `initTimeout: 120000`; the next start
    reused it offline at once.

## Still requires user action

- No package was published. Publish with the command in the
  [README](cadcraft-edge-mcp/README.md), platform packages first, then check all four versions.
- **LibreChat downloads:** the EdgeHomes fork (`feat/mcp-file-attachments`, checked at `46d877383`)
  offers MCP resources as downloads only for pdf, docx, pptx, xlsx, rtf and csv. **Neither
  `application/dxf` nor `application/acad` is on that list**, so a DXF/DWG `file_attach` is not
  offered as a download there until those two types are added to `DOWNLOADABLE_MIME_TYPES`
  (`packages/api/src/mcp/parsers.ts`). It also hands every `image/*` resource to the model as an
  image, which is why the IANA `image/vnd.dxf` / `image/vnd.dwg` types are not used.

Not tested or changed: real LibreChat chats and upload copying, model tool choice, the deployed
image, and restarting the service. What CADCraft models round-trips; content it doesn't model may
not survive (see `docs/file-format-parity.md`), and the upload itself is never changed. Per-user
roots are left to the deployment (the fork's `fileWorkspace`).
