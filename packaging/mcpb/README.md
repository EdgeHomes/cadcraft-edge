# CADCraft for Claude Desktop (MCP Bundle)

`cadcraft-<version>.mcpb` is a [MCP Bundle](https://github.com/modelcontextprotocol/mcpb) that
runs CADCraft's own MCP server (`cadcraft-cli mcp`) inside Claude Desktop on macOS and Windows.
It offers every tool (no `--compact`): `drawing_create`, `open`, `find_files`, `save`, `file_attach`,
`inspect_drawing`, `query_entities`, `find_text`, `command_line`, `script`, `execute`,
`list_commands`, `render`, `new_drawing` and `cancel`.

## Install

1. Open Claude Desktop, then **Settings → Extensions** (or double-click the `.mcpb` file).
2. Drag `cadcraft-<version>.mcpb` onto the window, or use **Advanced settings → Install
   Extension…** and choose it.
3. Choose the **Working folder** when asked. The default is `Documents/CADCraft` in your home
   folder. Click **Install**, then make sure the extension is enabled.
4. In a chat, ask for a drawing ("draw a 10 × 6 m two-room floor plan with dimensions") or name
   or drop a DXF/DWG file and ask for an edit.

## The working folder

The server is started as `cadcraft-cli mcp --root <working folder>` (unrestricted, like the
ChatGPT plugin).

- New drawings and relative paths go to the working folder (default `Documents/CADCraft`), which
  is made on first start.
- Nothing else is restricted: drawings anywhere on the computer can be opened and saved. Claude
  Desktop doesn't put a file dropped into the chat on your disk (it goes to the chat's sandbox),
  so the model finds the original by name with `find_files` (working folder, Desktop, Downloads,
  Documents, then the home folder, newest first). A small file it can't find (up to 2 MiB) can be
  sent as bytes: `open {data, name}`.

## Files in the bundle

| Path | What |
|---|---|
| `manifest.json` | MCPB manifest 0.3: binary server, `platform_overrides` for Windows, `user_config.working_folder`, the full tool list |
| `server/cadcraft-cli` | macOS universal binary (arm64 + x86_64) |
| `server/cadcraft-cli.exe` | Windows x86_64 binary (also runs on Windows on Arm, emulated) |
| `icon.png` | CADCraft's own app icon, 512 × 512 (`assets/app-icon/cadcraft-macos-512.png`) |
| `LICENSE-MIT`, `LICENSE-APACHE`, `NOTICE` | CADCraft's licences |

No fonts are embedded: drawings render with CADCraft's own stroke font, and a font a drawing
names that isn't on the computer is replaced in renders only (the file keeps its name).

## Build

```sh
packaging/mcpb/build.sh
```

It builds the macOS binaries for both architectures and joins them with `lipo`, cross-builds the
Windows binary with [`cargo-xwin`](https://github.com/rust-cross/cargo-xwin) (`cargo install
--locked cargo-xwin`; it downloads Microsoft's CRT and Windows SDK headers on first use), refreshes
`manifest.json`'s version and tools from `cadcraft-cli tools --json`, then runs `mcpb validate` and
`mcpb pack` (`@anthropic-ai/mcpb` 2.1.2, through `npx`). The result is
`dist/cadcraft-<version>.mcpb` (`dist/` is ignored by git). `MCPB_VERSION=0.17.1` versions the
bundle on its own, e.g. for a packaging-only release; the server binary keeps the workspace version.

After changing a tool, run `packaging/mcpb/build.sh --manifest-only` and commit `manifest.json`:
the test `the_mcpb_manifest_matches_the_tool_list` fails while they differ.

## macOS Gatekeeper and signing

The macOS binary is only ad-hoc signed (as the Rust linker signs every arm64 binary): it is not
signed with a Developer ID or notarized, and this repository has no Developer ID signing set up
for it (`packaging/macos/` signs the app bundle in CI from secrets). Gatekeeper blocks a program
that carries the `com.apple.quarantine` attribute and isn't notarized. On the machine this was
built on, the files of an extension Claude Desktop had installed carried only
`com.apple.provenance`, not quarantine, but that was not checked for this bundle inside Claude
Desktop (see `VERIFICATION.md`). If macOS refuses to run it ("cannot be opened because the
developer cannot be verified"), sign and notarize before distributing:

```sh
codesign --force --options runtime --timestamp --sign "Developer ID Application: <Team>" dist/mcpb/stage/server/cadcraft-cli
# then repack, zip the binary, and: xcrun notarytool submit <zip> --keychain-profile <profile> --wait
```

A notarized command-line binary can't be stapled; Gatekeeper checks it online on first run. Do not
work around Gatekeeper by removing the quarantine attribute or turning off its checks. Windows
SmartScreen may likewise warn about an unsigned `.exe`; `packaging/windows/sign.ps1` signs with an
Authenticode certificate. See `VERIFICATION.md` for what was tested where.
