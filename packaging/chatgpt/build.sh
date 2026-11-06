#!/bin/sh
# Build the CADCraft plugin for ChatGPT Desktop: dist/cadcraft-chatgpt-<version>.zip, ready for
# the workspace's Add → Upload plugin.
#
#   packaging/chatgpt/build.sh        (PLUGIN_VERSION=x.y.z versions the plugin on its own)
#
# The zip holds the plugin at its root: plugin.json + mcp.json (Agent Plugins) and
# .codex-plugin/plugin.json + .mcp.json (Codex), assets/icon.png, the licences, and
# server/cadcraft-cli (macOS universal) beside server/cadcraft-cli.exe (Windows x86_64). Both
# configs start `./server/cadcraft-cli mcp --root ~/Documents/CADCraft`; Windows adds `.exe` to a
# command without an extension. Same binaries and tools as packaging/mcpb/build.sh.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"
target_dir="${CARGO_TARGET_DIR:-$repo/target}"
version=${PLUGIN_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -n 1)}
[ -n "$version" ] || { echo "build.sh: no workspace version in Cargo.toml" >&2; exit 1; }
[ -z "${CRAFT_FONTS_DIR:-}" ] || { echo "build.sh: unset CRAFT_FONTS_DIR (embedded fonts need their licences shipped)" >&2; exit 1; }
timeout_run() { perl -e 'alarm shift; exec @ARGV' "$@"; }

for t in aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc; do
  rustup target list --installed | grep -qx "$t" || rustup target add "$t" >/dev/null
done
cd "$repo"
timeout_run 3600 cargo build -p cadcraft-cli --release --locked --target aarch64-apple-darwin
timeout_run 3600 cargo build -p cadcraft-cli --release --locked --target x86_64-apple-darwin
timeout_run 3600 cargo xwin build -p cadcraft-cli --release --locked --target x86_64-pc-windows-msvc

stage="$repo/dist/chatgpt/stage"
rm -rf "$stage" && mkdir -p "$stage/server" "$stage/assets"
/bin/cp -Rf "$here/plugin/." "$stage/"
lipo -create -output "$stage/server/cadcraft-cli" \
  "$target_dir/aarch64-apple-darwin/release/cadcraft-cli" "$target_dir/x86_64-apple-darwin/release/cadcraft-cli"
/bin/cp -f "$target_dir/x86_64-pc-windows-msvc/release/cadcraft-cli.exe" "$stage/server/cadcraft-cli.exe"
# CADCraft's own icon (ATTRIBUTION.md), and the project's licence texts.
/bin/cp -f "$repo/assets/app-icon/cadcraft-macos-512.png" "$stage/assets/icon.png"
/bin/cp -f "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$repo/NOTICE" "$stage/"
/bin/cp -f "$here/README.md" "$stage/README.md"

out="$repo/dist/cadcraft-chatgpt-$version.zip"
python3 - "$stage" "$out" "$version" <<'EOF'
import json, os, sys, zipfile
stage, out, version = sys.argv[1:]
for m in ("plugin.json", ".codex-plugin/plugin.json"):
    p = os.path.join(stage, m)
    d = json.load(open(p))
    d["version"] = version
    open(p, "w").write(json.dumps(d, indent=2) + "\n")
if os.path.exists(out):
    os.remove(out)
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED, compresslevel=9) as z:
    for root, dirs, files in os.walk(stage):
        dirs.sort()
        for f in sorted(files):
            if f in (".DS_Store", "Thumbs.db"):
                continue
            path = os.path.join(root, f)
            name = os.path.relpath(path, stage).replace(os.sep, "/")
            info = zipfile.ZipInfo(name, (2026, 1, 1, 0, 0, 0))
            info.compress_type = zipfile.ZIP_DEFLATED
            # The binaries keep their execute bit when unpacked.
            info.external_attr = (0o100755 if name.startswith("server/") else 0o100644) << 16
            info.create_system = 3
            z.writestr(info, open(path, "rb").read())
size = os.path.getsize(out)
assert size < 100 * 1024 * 1024, f"{out} is {size} bytes, over 100 MiB"
EOF
ls -l "$out"
