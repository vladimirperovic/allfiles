#!/usr/bin/env bash
# Optional destination prefix; sourcing also exports the library paths for cargo.
set -euo pipefail

allfiles_sqlite_dir="${1:-$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)/.native-sqlite}"
mkdir -p "$allfiles_sqlite_dir"
allfiles_sqlite_dir="$(cd -- "$allfiles_sqlite_dir" && pwd)"
allfiles_sqlite_archive="${allfiles_sqlite_dir}/sqlite-amalgamation-3530400.zip"
mkdir -p "${allfiles_sqlite_dir}/include" "${allfiles_sqlite_dir}/lib/pkgconfig"
curl --fail --location --retry 3 --proto '=https' --tlsv1.2 \
  'https://sqlite.org/2026/sqlite-amalgamation-3530400.zip' \
  --output "$allfiles_sqlite_archive"
printf '%s  %s\n' '1e71ddf93849c6a6ecf58b827c0692073d2dd7ee40196158068f7b29f422e87d' "$allfiles_sqlite_archive" | sha256sum --check --status
unzip -qo "$allfiles_sqlite_archive" -d "$allfiles_sqlite_dir"
allfiles_sqlite_source="${allfiles_sqlite_dir}/sqlite-amalgamation-3530400"
"${CC:-cc}" -O2 -fPIC -DSQLITE_THREADSAFE=1 -DSQLITE_ENABLE_FTS5 \
  -c "${allfiles_sqlite_source}/sqlite3.c" -o "${allfiles_sqlite_dir}/sqlite3.o"
"${AR:-ar}" rcs "${allfiles_sqlite_dir}/lib/libsqlite3.a" "${allfiles_sqlite_dir}/sqlite3.o"
cp "${allfiles_sqlite_source}/sqlite3.h" "${allfiles_sqlite_source}/sqlite3ext.h" "${allfiles_sqlite_dir}/include/"
# libsqlite3-sys probes pkg-config before emitting SQLITE3_LIB_DIR itself. Supply
# our own metadata so an installed system sqlite3.pc cannot silently win.
cat > "${allfiles_sqlite_dir}/lib/pkgconfig/sqlite3.pc" <<'EOF'
prefix=${pcfiledir}/../..
exec_prefix=${prefix}
libdir=${prefix}/lib
includedir=${prefix}/include

Name: SQLite
Description: Pinned native SQLite for the AllFiles search comparison
Version: 3.53.4
Libs: -L${libdir} -lsqlite3
Libs.private: -lm -ldl -lpthread
Cflags: -I${includedir}
EOF
export SQLITE3_LIB_DIR="${allfiles_sqlite_dir}/lib"
export SQLITE3_INCLUDE_DIR="${allfiles_sqlite_dir}/include"
export SQLITE3_STATIC=1
