#!/usr/bin/env bash
#
# Run every check CI runs, with the same commands, so a developer and a workflow run one thing.
# Fails on the first failing command.
set -euo pipefail

readonly script_name="test.sh"

usage() {
  cat <<EOF
Usage: scripts/$script_name [--no-deny]

Runs cargo fmt --check, cargo clippy --locked --all-targets --all-features -- -D warnings,
cargo test --locked --all-features (doc tests included) and cargo deny check.

  --no-deny    Skip cargo deny, for a machine without it installed. CI never passes this.
  -h, --help   Show this message.
EOF
}

fail() {
  echo "$script_name: $*" >&2
  exit 1
}

run_deny=true
while [ "$#" -gt 0 ]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    --no-deny)
      run_deny=false
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
  shift
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo test --locked --all-features
if [ "$run_deny" = true ]; then
  # Asked through cargo, which also finds subcommands in ~/.cargo/bin when that is not on PATH.
  cargo deny --version >/dev/null 2>&1 || fail "cargo-deny is not installed (cargo install cargo-deny, or pass --no-deny)"
  cargo deny check
fi
