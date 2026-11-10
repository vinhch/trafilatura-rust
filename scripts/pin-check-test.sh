#!/usr/bin/env bash
#
# Runs scripts/pin-check.sh on generated manifests, one per case: an exact requirement in each dependency
# table form (string, inline table, spaced, dotted key, dev, build, target, sub-table), requirements and
# package keys that are not exact, comments and other tables, the repo Cargo.toml, a missing file and the
# -h and unknown-flag exits. Manifests are written under .temp/pin-check-test/ on every run.
# pin-check-test.ps1 is the twin.
set -euo pipefail

readonly script_name="pin-check-test.sh"

usage() {
  cat <<EOF
Usage: scripts/$script_name

Runs scripts/pin-check.sh on generated manifests and checks the exit code and messages of each case.

  -h, --help   Show this message.
EOF
}

fail() {
  echo "$script_name: $*" >&2
  exit 1
}

if [ "$#" -gt 0 ]; then
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

work=".temp/pin-check-test"
rm -rf "$work"
mkdir -p "$work"

failures=0
passes=0

# manifest <name>: writes the manifest of a case from stdin to $work/<name>.toml.
manifest() {
  cat >"$work/$1.toml"
}

# run_case <name> <expected exit> <expected stderr fragment or empty> <args...>: runs the script and checks
# the exit code and that stderr holds the fragment (empty: stderr must be empty).
run_case() {
  local name="$1" want_exit="$2" want_err="$3" rc=0 problems=""
  shift 3
  scripts/pin-check.sh "$@" >"$work/out-$name" 2>"$work/err-$name" || rc=$?
  [ "$rc" -eq "$want_exit" ] || problems+="exit $rc, want $want_exit; "
  if [ -n "$want_err" ]; then
    grep -Fq -- "$want_err" "$work/err-$name" || problems+="stderr lacks '$want_err'; "
  else
    [ ! -s "$work/err-$name" ] || problems+="stderr '$(cat "$work/err-$name")', want none; "
  fi
  if [ -z "$problems" ]; then
    passes=$((passes + 1))
  else
    echo "pin-check-test.sh: case $name: $problems" >&2
    failures=$((failures + 1))
  fi
}

# exact_case <name> <line number of the hit>: the manifest was just written; the hit is reported as
# <manifest>:<line>: exact requirement.
exact_case() {
  run_case "$1" 1 "pin-check.sh: $work/$1.toml:$2: exact requirement: " "$work/$1.toml"
}

manifest exact_string <<'EOF'
[package]
name = "x"

[dependencies]
foo = "=1.2"
EOF
exact_case exact_string 5

manifest exact_table <<'EOF'
[package]
name = "x"

[dependencies]
foo = { version = "=1.2", default-features = false }
EOF
exact_case exact_table 5

manifest exact_table_later_key <<'EOF'
[package]
name = "x"

[dependencies]
foo = { path = "../foo", version = "=1.2" }
EOF
exact_case exact_table_later_key 5

manifest exact_spaced <<'EOF'
[package]
name = "x"

[dependencies]
foo = "= 1.2"
EOF
exact_case exact_spaced 5

manifest exact_single_quote <<'EOF'
[package]
name = "x"

[dependencies]
foo = '=1.2'
EOF
exact_case exact_single_quote 5

manifest exact_dotted_key <<'EOF'
[package]
name = "x"

[dependencies]
foo.version = "=1.2"
EOF
exact_case exact_dotted_key 5

manifest exact_dev <<'EOF'
[package]
name = "x"

[dev-dependencies]
foo = "=1.2"
EOF
exact_case exact_dev 5

manifest exact_build <<'EOF'
[package]
name = "x"

[build-dependencies]
foo = "=1.2"
EOF
exact_case exact_build 5

manifest exact_target <<'EOF'
[package]
name = "x"

[target.'cfg(unix)'.dependencies]
foo = "=1.2"
EOF
exact_case exact_target 5

manifest exact_target_dev <<'EOF'
[package]
name = "x"

[target.'cfg(unix)'.dev-dependencies]
foo = "=1.2"
EOF
exact_case exact_target_dev 5

manifest exact_subtable <<'EOF'
[package]
name = "x"

[dependencies.foo]
version = "=1.2"
EOF
exact_case exact_subtable 5

manifest exact_after_ok_lines <<'EOF'
[package]
name = "x"

[dependencies]
bar = "1"
# foo = "=0.1"
baz = { version = "2" }
foo = "=1.2"
EOF
exact_case exact_after_ok_lines 8

manifest caret_ok <<'EOF'
[package]
name = "x"

[dependencies]
foo = "1.2"
bar = { version = ">=1, <2" }
baz = { version = "~1.2", optional = true }
[dependencies.qux]
version = "1"
features = ["=x"]
EOF
run_case caret_ok 0 "" "$work/caret_ok.toml"

manifest package_keys_ok <<'EOF'
[package]
name = "x"
version = "0.1.0"
rust-version = "1.98.1"

[package.metadata.upstream]
version = "2.3.1"
commit = "e2f85c965662c7df26b73290ef175b2c2a5fdca6"
EOF
run_case package_keys_ok 0 "" "$work/package_keys_ok.toml"

manifest other_tables_ok <<'EOF'
[package]
name = "x"

[features]
foo = ["=x"]

[package.metadata.docs.rs]
version = "=1"

[dependencies]
bar = "1"

[lints.rust]
version = "=1"
EOF
run_case other_tables_ok 0 "" "$work/other_tables_ok.toml"

manifest comment_ok <<'EOF'
[dependencies]
# foo = "=1.2"
   # bar = { version = "=1" }
baz = "1"
EOF
run_case comment_ok 0 "" "$work/comment_ok.toml"

run_case real_manifest 0 "" Cargo.toml
run_case default_manifest 0 ""
run_case help 0 "" -h
run_case unknown_flag 1 "unknown argument: --bogus" --bogus
run_case missing_manifest 1 "$work/nope.toml: no such file" "$work/nope.toml"

# -h prints the usage on stdout.
case "$(head -n 1 "$work/out-help")" in
  Usage:*) passes=$((passes + 1)) ;;
  *)
    echo "pin-check-test.sh: case help: first stdout line '$(head -n 1 "$work/out-help")', want prefix 'Usage:'" >&2
    failures=$((failures + 1))
    ;;
esac

if [ "$failures" -gt 0 ]; then
  exit 1
fi
echo "pin-check-test: $passes cases passed"
