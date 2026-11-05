#!/usr/bin/env bash
#
# Check hand-written .rs files against the file size guide in AGENTS.md (File size guide): 800 lines for a
# production file, 1,000 for a test file, and 1.5 times either as a hard cap. Past the guide a file needs a
# size: line in its leading block. Generated files (@generated on line 1) are skipped.
# size-check.ps1 is the twin: same output and exit codes.
set -euo pipefail

readonly script_name="size-check.sh"

usage() {
  cat <<EOF
Usage: scripts/$script_name [FILE...]

Checks the .rs files git tracks, or the FILEs given (relative to the repo root), against the file size guide
in AGENTS.md. Exits 1 on any violation.

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

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

# Prints "<lines> <generated 0|1> <reason 0|1>". The record count is the line count: the newline count,
# plus one for a last line without a newline.
file_stats() {
  awk '
    {
      line = $0
      sub(/\r$/, "", line)
      if (NR == 1 && index(line, "@generated") > 0) generated = 1
      if (done) next
      text = line
      sub(/^ +/, "", text)
      if (open_attr) {
        if (text ~ /\]$/) open_attr = 0
      } else if (text == "" || text ~ /^\/\// || text ~ /^#!\[/) {
        if (text ~ /^#!\[/ && text !~ /\]$/) open_attr = 1
      } else {
        done = 1
        next
      }
      if (text ~ /^\/\/!/) has_moddoc = 1
      if (text ~ /^\/\/! size: [^[:space:]]/) doc_reason = 1
      if (text ~ /^\/\/ size: [^[:space:]]/) slash_reason = 1
    }
    END {
      reason = (doc_reason || (!has_moddoc && slash_reason)) ? 1 : 0
      printf "%d %d %d\n", NR, generated + 0, reason
    }
  ' "$1"
}

# Test files sit under a tests/ directory or are a tests.rs module file.
kind_of() {
  case "$1" in
    tests/* | */tests/* | tests.rs | */tests.rs) echo test ;;
    *) echo production ;;
  esac
}

if [ "$#" -gt 0 ]; then
  paths=("$@")
else
  listed="$(git ls-files -- '*.rs')"
  paths=()
  while IFS= read -r path; do
    if [ -n "$path" ]; then
      paths+=("$path")
    fi
  done <<<"$listed"
fi

checked=0
generated=0
violations=0
for path in "${paths[@]}"; do
  if [ ! -f "$path" ]; then
    echo "$script_name: $path: no such file" >&2
    violations=$((violations + 1))
    continue
  fi
  stats="$(file_stats "$path")"
  read -r lines is_generated has_reason <<<"$stats"
  if [ "$is_generated" -eq 1 ]; then
    generated=$((generated + 1))
    continue
  fi
  checked=$((checked + 1))
  kind="$(kind_of "$path")"
  if [ "$kind" = test ]; then
    cap=1000
  else
    cap=800
  fi
  hard=$((cap + cap / 2))
  if [ "$lines" -gt "$hard" ]; then
    echo "$script_name: $path: $lines lines, over the $hard-line $kind hard cap" >&2
    violations=$((violations + 1))
  elif [ "$lines" -gt "$cap" ] && [ "$has_reason" -eq 0 ]; then
    echo "$script_name: $path: $lines lines, over the $cap-line $kind cap without a size: line" >&2
    violations=$((violations + 1))
  fi
done

if [ "$violations" -gt 0 ]; then
  exit 1
fi
if [ "$checked" -eq 0 ]; then
  fail "no hand-written .rs file checked"
fi
echo "size-check: $checked files checked, $generated generated skipped"
