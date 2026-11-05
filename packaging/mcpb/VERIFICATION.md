# Claude Desktop bundle (.mcpb) verification

Date: 2026-11-05. Version **0.17.0**. Toolchain: Rust **1.97.1**; `@anthropic-ai/mcpb` **2.1.2**
(manifest spec 0.3, the spec's current version); `cargo-xwin` 0.23.1. No fonts embedded. This
records exactly what was checked and where. Shared server checks (tests, regression proofs,
design review) are in [`../npm/VERIFICATION.md`](../npm/VERIFICATION.md).

## The bundle

`packaging/mcpb/build.sh` → `dist/cadcraft-0.17.0.mcpb`, **40.7 MB** (42,649,371 bytes; 103.5 MB unpacked, 8 files; SHA-256 `f1eda1a587c4b95b18a83a5be57a38f4a8296380a21826a835d06022a17e8a46`):

| File | Check |
|---|---|
| `server/cadcraft-cli` | `file`: Mach-O universal binary, x86_64 + arm64; `codesign -dv`: ad-hoc, linker-signed (no Developer ID) |
| `server/cadcraft-cli.exe` | `file`: PE32+ executable (console) x86-64, for MS Windows |
| `manifest.json` | `mcpb validate`: schema passes; icon passes (512 × 512) |
| `icon.png` | `assets/app-icon/cadcraft-macos-512.png`, CADCraft's own (ATTRIBUTION.md) |
| `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE`, `README.md` | |

What dominates the size: the universal macOS binary (71.5 MB: two ~35 MB slices) and the Windows
binary (37.0 MB), all CADCraft's engine, DWG bridge, renderer and PDF backends. No fonts or other
data are embedded; the icon and licence texts are about 50 KB.

## Runtime

| Where | What ran | Result |
|---|---|---|
| macOS 27.2 (arm64) | The `server/cadcraft-cli` **unpacked from the .mcpb**, arm64 slice, through the stdio script below, once as Claude Desktop starts it (full tool list, `--create-root --root`) and once with `--compact` | Passed both workflows |
| macOS x86_64 slice | — | **Not run**: this Mac has no Rosetta 2, and none was installed for this check |
| Windows | — | **Not verified at runtime**: no Windows machine or VM was available. Only `file` was checked, which doesn't count as a runtime check |
| Claude Desktop (macOS) | — | **Not installed in the app**: installing needs the person at the computer to confirm Claude Desktop's install dialog. See "In-app check" below |

The stdio script (both workflows, fed as request files on stdin so it needs nothing where the binary
runs, replies checked on the host):

1. `initialize`: the instructions start "CADCraft makes and edits CAD drawings. To make a new
   drawing, call drawing_create."; `tools/list` has 14 tools.
2. One `drawing_create` call: `Site Plan.dxf`, plot scale 1:50, nothing dropped. The embedded
   resource is `file:///Site%20Plan.dxf`, `application/dxf`, and `file_attach` returns the same
   bytes. `render` returns a PNG with no display.
3. The file is copied to `inputs/5d0c9a1e/Site Plan.dxf` (LibreChat's upload layout). A new server
   opens that exact absolute path, finds `BEDROOM` with `find_text`, changes it with `textedit`,
   adds a layer with `-layer`, saves with no path (`Site Plan (2).dxf`) and attaches it.
   `file_attach` of `/etc/hostname` is refused (outside the root). The upload's SHA-256 is
   unchanged.
4. A third server reopens the edited file: the edit is there; both texts, the dimension, the
   hatch, the `Notes` layer and the `Chair` block are present.

Identical files on every run: created
`b8a3583caed825243d96184c5e7c5a5de88e6bc32ed1aa93b7e61f307a6fb690`, edited
`89ba397c7bdc6e8c121d57bdd241aa039f887c2d334efc4ab7a49770add44b3d`.

`--create-root --root <absolute>/Documents/CADCraft` made the missing folder; a relative or empty
value is refused (exit 1), so the server never runs unconfined (`create_root_…` test).

## Gatekeeper and signing

- The universal binary is ad-hoc signed by the linker; it is not Developer ID signed or notarized.
  This Mac's only code-signing identity is an "Apple Development" certificate, which can't
  notarize for distribution, and the repository has no Developer ID signing for the CLI.
- On this Mac, the files of an extension Claude Desktop had already installed
  (`Claude Extensions/local.mcpb.…`) carry `com.apple.provenance` and **no**
  `com.apple.quarantine`. That extension is Node-based, so this suggests, but does not prove, that
  Claude Desktop's unpacked binaries aren't quarantined.
- What you must do before distributing outside your own machines: sign
  `server/cadcraft-cli` with a Developer ID Application certificate (hardened runtime, timestamp),
  notarize it with `notarytool`, and repack; sign `cadcraft-cli.exe` with an Authenticode
  certificate (`packaging/windows/sign.ps1`) to avoid SmartScreen warnings. Don't strip quarantine
  attributes or disable Gatekeeper as a workaround.

## Windows paths

`crates/engine/src/file_access.rs` handles drive letters and UNC paths (compared by component
after `canonicalize`, with the `\\?\` prefix removed for messages), backslashes and forward
slashes, case-insensitive comparison, and refuses device names (`CON`, `NUL`, `COM1`…, also with
an extension). `windows_ignores_case_and_refuses_devices` runs only on Windows; it was compiled for
`x86_64-pc-windows-msvc` but **not run**. `device_names` runs everywhere.

## In-app check (to do on a Mac with Claude Desktop)

1. Double-click `dist/cadcraft-0.17.0.mcpb` (or Settings → Extensions → Install Extension…).
2. Keep the default working folder, install and enable.
3. Ask: "Draw a 10 by 6 metre two-room floor plan with dimensions." Expect one `drawing_create`
   call, a downloadable `.dxf`, and the file in `~/Documents/CADCraft`.
4. Copy a DXF into that folder, ask for an edit, and check the original is unchanged and a
   `name (2).dxf` appears.
5. If macOS blocks the binary, record the message: signing and notarization are then required.
