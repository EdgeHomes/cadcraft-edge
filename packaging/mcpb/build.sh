#!/bin/sh
# Build the CADCraft MCP Bundle for Claude Desktop: dist/cadcraft-<version>.mcpb.
#
#   packaging/mcpb/build.sh                  build both binaries, refresh the manifest, validate, pack
#   packaging/mcpb/build.sh --manifest-only  refresh packaging/mcpb/manifest.json's version and tools
#
# macOS: one universal cadcraft-cli (arm64 + x86_64, lipo). Windows: x86_64 cadcraft-cli.exe built
# with cargo-xwin (MSVC CRT and Windows SDK; `cargo install --locked cargo-xwin`), which also runs
# on Windows on Arm through emulation. Needs Node.js for @anthropic-ai/mcpb, pinned below.
# Toolchain: RUSTUP_TOOLCHAIN (default 1.97.1). No fonts are embedded unless CRAFT_FONTS_DIR is set.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"
target_dir="${CARGO_TARGET_DIR:-$repo/target}"
MCPB="npx -y @anthropic-ai/mcpb@2.1.2"
# MCPB_VERSION versions the bundle on its own (e.g. a packaging-only fix); default: the workspace's.
version=${MCPB_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -n 1)}
[ -n "$version" ] || { echo "build.sh: no workspace version in Cargo.toml" >&2; exit 1; }
timeout_run() { perl -e 'alarm shift; exec @ARGV' "$@"; }

# The manifest's version and tool list come from the server itself (`tools --json`); a test
# (`the_mcpb_manifest_matches_the_tool_list`) fails when they drift apart.
refresh_manifest() {
  cli=$1
  "$cli" tools --json | python3 -c '
import json, sys
tools = json.load(sys.stdin)
path = sys.argv[1]
m = json.load(open(path))
m["version"] = sys.argv[2]
m["tools"] = [{"name": t["name"], "description": t["description"]} for t in tools["tools"]]
open(path, "w").write(json.dumps(m, indent=2, ensure_ascii=False) + "\n")
' "$here/manifest.json" "$version"
}

if [ "${1:-}" = "--manifest-only" ]; then
  (cd "$repo" && timeout_run 1800 cargo build -p cadcraft-cli --locked)
  refresh_manifest "$target_dir/debug/cadcraft-cli"
  echo "refreshed $here/manifest.json"
  exit 0
fi

for t in aarch64-apple-darwin x86_64-apple-darwin x86_64-pc-windows-msvc; do
  rustup target list --installed | grep -qx "$t" || rustup target add "$t" >/dev/null
done
cd "$repo"
timeout_run 3600 cargo build -p cadcraft-cli --release --locked --target aarch64-apple-darwin
timeout_run 3600 cargo build -p cadcraft-cli --release --locked --target x86_64-apple-darwin
timeout_run 3600 cargo xwin build -p cadcraft-cli --release --locked --target x86_64-pc-windows-msvc

stage="$repo/dist/mcpb/stage"
rm -rf "$stage" && mkdir -p "$stage/server"
lipo -create -output "$stage/server/cadcraft-cli" \
  "$target_dir/aarch64-apple-darwin/release/cadcraft-cli" "$target_dir/x86_64-apple-darwin/release/cadcraft-cli"
/bin/cp -f "$target_dir/x86_64-pc-windows-msvc/release/cadcraft-cli.exe" "$stage/server/cadcraft-cli.exe"
refresh_manifest "$stage/server/cadcraft-cli"
/bin/cp -f "$here/manifest.json" "$stage/manifest.json"
# CADCraft's own icon (ATTRIBUTION.md), and the project's licence texts.
/bin/cp -f "$repo/assets/app-icon/cadcraft-macos-512.png" "$stage/icon.png"
/bin/cp -f "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$repo/NOTICE" "$stage/"
/bin/cp -f "$here/README.md" "$stage/README.md"
if [ -n "${CRAFT_FONTS_DIR:-}" ]; then
  echo "build.sh: CRAFT_FONTS_DIR is set; add the embedded font families' licence texts to the bundle" >&2
  exit 1
fi

$MCPB validate "$stage/manifest.json"
out="$repo/dist/cadcraft-$version.mcpb"
rm -f "$out"
$MCPB pack "$stage" "$out"
ls -l "$out"
