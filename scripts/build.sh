#!/bin/sh
# Cross-compile standalone executables. Requires the Go version from go.mod.
set -eu

script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
project_dir=$(CDPATH= cd -- "$script_dir/.." && pwd)
cd "$project_dir"

version=${1:-0.1.0}
case "$version" in
  *[!a-zA-Z0-9._+-]*|'')
    printf '%s\n' 'Version must contain only letters, digits, dots, underscores, plus or minus signs.' >&2
    exit 1
    ;;
esac

mkdir -p dist
for target in linux/amd64 linux/arm64 darwin/amd64 darwin/arm64 windows/amd64; do
  target_os=${target%/*}
  target_arch=${target#*/}
  extension=
  if [ "$target_os" = windows ]; then extension=.exe; fi
  output="dist/allfiles-${target_os}-${target_arch}${extension}"
  printf 'Building %s\n' "$target"
  CGO_ENABLED=0 GOOS="$target_os" GOARCH="$target_arch" \
    go build -trimpath -ldflags="-s -w -X main.Version=$version" -o "$output" .
done
cp README.md THIRD_PARTY_NOTICES.txt dist/
printf '%s\n' 'Binaries are in dist/. No catalogs or document content are included.'
