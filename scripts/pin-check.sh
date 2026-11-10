#!/usr/bin/env bash
#
# Fail on a dependency requirement that starts with `=` in Cargo.toml: an exact pin keeps a dependency at one
# version and blocks the security fixes Dependabot and cargo update would bring. Reads [dependencies],
# [dev-dependencies], [build-dependencies], their [target.<spec>.*] forms and [dependencies.<name>] sub-tables;
# other tables (package, metadata, features) are never checked. A requirement is a bare string value, a
# version = inside an inline table, or a version line of a sub-table. Multi-line inline tables are out of
# scope: none exists in the repo.
# pin-check.ps1 is the twin: same output and exit codes.
set -euo pipefail

readonly script_name="pin-check.sh"

usage() {
  cat <<EOF
Usage: scripts/$script_name [MANIFEST]

Checks the dependency requirements of MANIFEST (relative to the repo root, default Cargo.toml) and exits 1
when one starts with "=".

  -h, --help   Show this message.
EOF
}

fail() {
  echo "$script_name: $*" >&2
  exit 1
}

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  -*)
    fail "unknown argument: $1"
    ;;
esac
[ "$#" -le 1 ] || fail "unknown argument: $2"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

manifest="${1:-Cargo.toml}"
[ -f "$manifest" ] || fail "$manifest: no such file"

# One line per hit, in file order.
hits="$(awk -v name="$script_name" -v file="$manifest" '
  {
    line = $0
    sub(/\r$/, "", line)
    text = line
    sub(/^[ \t]+/, "", text)
    if (text ~ /^\[\[/) {
      mode = ""
      next
    }
    if (text ~ /^\[/) {
      table = text
      sub(/^\[[ \t]*/, "", table)
      sub(/[ \t]*\][ \t]*(#.*)?$/, "", table)
      mode = ""
      if (table ~ /(^|\.)(dev-|build-)?dependencies$/) mode = "plain"
      else if (table ~ /(^|\.)(dev-|build-)?dependencies\.[^.]+$/) mode = "sub"
      next
    }
    if (text ~ /^#/ || mode == "") next
    q = "[\"\047][ \t]*="
    hit = 0
    if (mode == "sub") {
      if (text ~ ("^version[ \t]*=[ \t]*" q)) hit = 1
    } else if (text ~ ("^[A-Za-z0-9_-]+[ \t]*=[ \t]*" q) ||
               text ~ ("^[A-Za-z0-9_-]+\\.version[ \t]*=[ \t]*" q) ||
               text ~ ("[{,][ \t]*version[ \t]*=[ \t]*" q)) {
      hit = 1
    }
    if (hit) printf "%s: %s:%d: exact requirement: %s\n", name, file, NR, text
  }
' "$manifest")"

if [ -n "$hits" ]; then
  echo "$hits" >&2
  exit 1
fi
echo "pin-check: $manifest: no exact requirement"
