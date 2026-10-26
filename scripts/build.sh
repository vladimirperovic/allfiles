#!/bin/sh
# Native build, or an installed Rust target with its matching C linker.
set -eu
script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$script_dir/.."
target=${1:-$(rustc -vV | sed -n 's/^host: //p')}
case "$target" in
  x86_64-unknown-linux-*) name=allfiles-linux-amd64; extension= ;;
  aarch64-unknown-linux-*) name=allfiles-linux-arm64; extension= ;;
  x86_64-apple-darwin) name=allfiles-darwin-amd64; extension= ;;
  aarch64-apple-darwin) name=allfiles-darwin-arm64; extension= ;;
  x86_64-pc-windows-*) name=allfiles-windows-amd64.exe; extension=.exe ;;
  *) printf 'Unsupported target: %s\n' "$target" >&2; exit 1 ;;
esac
cargo build --release --locked --target "$target"
mkdir -p dist
cp "${CARGO_TARGET_DIR:-target}/$target/release/allfiles$extension" "dist/$name"
cp README.md README.en.md THIRD_PARTY_NOTICES.txt dist/
printf 'Built dist/%s. Catalogs are not included.\n' "$name"
