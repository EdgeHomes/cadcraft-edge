#!/bin/sh
# Stage the cadcraft-edge-mcp npm packages under packaging/npm/dist.
#
#   packaging/npm/stage.sh [RUST_TARGET ...]   (default: darwin-arm64 and both static Linux musl targets)
#
# Each target becomes a platform package (cadcraft-edge-mcp-<os>-<cpu>) holding the release
# cadcraft-cli binary, package.json and licenses/ (CADCraft's licence texts). No fonts are
# embedded: the binary draws text with CADCraft's own stroke font, and craft-fonts is not a build
# input here. The launcher package is staged last, with optionalDependencies on every platform
# package present in dist, all at the workspace version. Publish the platform packages first:
#   cd packaging/npm/dist && for d in cadcraft-edge-mcp-*/ cadcraft-edge-mcp/; do npm publish "$d" --access public || break; done
set -eu
here=$(cd "$(dirname "$0")" && pwd)
repo=$(cd "$here/../.." && pwd)
dist="$here/dist"
export RUSTUP_TOOLCHAIN="${RUSTUP_TOOLCHAIN:-1.97.1}"
# NPM_VERSION overrides the workspace version, for npm-only releases of the same engine version.
version=${NPM_VERSION:-$(sed -n 's/^version = "\(.*\)"/\1/p' "$repo/Cargo.toml" | head -n 1)}
[ -n "$version" ] || { echo "stage.sh: no workspace version in Cargo.toml" >&2; exit 1; }
[ -z "${CRAFT_FONTS_DIR:-}" ] || { echo "stage.sh: unset CRAFT_FONTS_DIR (embedded fonts would need their licence texts in licenses/)" >&2; exit 1; }

[ $# -gt 0 ] || set -- aarch64-apple-darwin x86_64-unknown-linux-musl aarch64-unknown-linux-musl

for target in "$@"; do
  case "$target" in
    x86_64-apple-darwin) os=darwin cpu=x64 ;;
    aarch64-apple-darwin) os=darwin cpu=arm64 ;;
    # Static musl builds only: one binary runs on Alpine (LibreChat's image) and glibc distros.
    x86_64-unknown-linux-musl) os=linux cpu=x64 ;;
    aarch64-unknown-linux-musl) os=linux cpu=arm64 ;;
    *) echo "stage.sh: unsupported target $target (Linux packages are musl only)" >&2; exit 1 ;;
  esac
  # Linux from macOS needs a cross linker: cargo-zigbuild (cargo install cargo-zigbuild; brew install zig).
  build=build
  case "$target" in *-linux-*) [ "$(uname -s)" = Linux ] || build=zigbuild ;; esac
  rustup target list --installed | grep -qx "$target" || rustup target add "$target" >/dev/null
  (cd "$repo" && perl -e 'alarm 3600; exec @ARGV' cargo "$build" -p cadcraft-cli --release --locked --target "$target")
  bin="${CARGO_TARGET_DIR:-$repo/target}/$target/release/cadcraft-cli"
  case "$target" in
    *-linux-musl) file "$bin" | grep -q 'statically linked' || { echo "stage.sh: $bin is not statically linked" >&2; exit 1; } ;;
  esac
  pkg="$dist/cadcraft-edge-mcp-$os-$cpu"
  rm -rf "$pkg" && mkdir -p "$pkg/bin" "$pkg/licenses"
  /bin/cp -f "$bin" "$pkg/bin/cadcraft-cli"
  /bin/cp -f "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$repo/NOTICE" "$pkg/licenses/"
  cat > "$pkg/package.json" <<EOF
{
  "name": "cadcraft-edge-mcp-$os-$cpu",
  "version": "$version",
  "description": "cadcraft-cli binary for cadcraft-edge-mcp ($os-$cpu)",
  "license": "MIT OR Apache-2.0",
  "repository": { "type": "git", "url": "git+https://github.com/mbarker-edgehomes/cadcraft-edge.git" },
  "os": ["$os"],
  "cpu": ["$cpu"],
  "files": ["bin", "licenses"]
}
EOF
  echo "staged $pkg ($target)"
done

main="$dist/cadcraft-edge-mcp"
rm -rf "$main" && /bin/cp -f -R "$here/cadcraft-edge-mcp" "$main"
/bin/cp -f "$repo/LICENSE-MIT" "$repo/LICENSE-APACHE" "$main/"
(cd "$main" && npm pkg set version="$version")
for p in "$dist"/cadcraft-edge-mcp-*/; do
  name=$(basename "$p")
  (cd "$main" && npm pkg set "optionalDependencies.$name=$version")
done
echo "staged $main ($version)"
