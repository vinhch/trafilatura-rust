#!/usr/bin/env bash
#
# Run every check CI runs, with the same commands, so a developer and a workflow run one thing.
# Fails on the first failing command.
set -euo pipefail

readonly script_name="test.sh"

# Tests that prove recursion fits a 512 KiB stack (AGENTS.md, Runtime rules). A new one goes on this list.
readonly stack_tests=(
  baseline::tests::deep_json_on_small_stack
  dom::tests::iter::deep_chain_walks_iteratively
  extraction::tests::caller_tree::deep_caller_tree_on_small_stack
  main_extractor::tests::dispatch::deep_nesting_fits_512k_stack
  pyjson::tests::deep_document_drops_on_small_stack
  xml::tests::output_deep_nesting_fits_512k_stack
  xml::tests::remove_blank_text_deep_nesting_fits_512k_stack
  options_discard::test_extract_deep_caller_supplied_tree
)

usage() {
  cat <<EOF
Usage: scripts/$script_name [--no-deny]

Runs scripts/version.sh --check, the size check (scripts/size-check.sh), cargo fmt --check,
cargo clippy --locked --all-targets --all-features -- -D warnings,
cargo clippy --locked -- -D warnings and with --no-default-features (the library alone, which warns about
items only the tests use), cargo test --locked --all-features (doc tests included), the batch example (examples/batch), the stack tests again
in the dev profile (opt-level 0, a real debug build) and cargo deny check.

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

scripts/version.sh --check
scripts/size-check-test.sh
scripts/size-check.sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo clippy --locked -- -D warnings
cargo clippy --locked --no-default-features -- -D warnings
cargo test --locked --all-features
# The batch example runs the public API over the committed pages in examples/batch (docs/batch.md).
cargo run --locked --example batch -- examples/batch
# The test profile builds at opt-level 1; the dev profile stays at 0, where stack frames are largest.
# A filter that matches nothing would pass vacuously, so the selection is counted first. An #[ignore]d stack test still runs here.
stack_args=(--locked --profile dev --all-features --lib --test unit_tests -- --include-ignored --exact "${stack_tests[@]}")
stack_listing="$(cargo test "${stack_args[@]}" --list)"
stack_found="$(grep -c ': test$' <<<"$stack_listing" || true)"
[ "$stack_found" -eq "${#stack_tests[@]}" ] || fail "stack tests: expected ${#stack_tests[@]}, found $stack_found"
cargo test "${stack_args[@]}"
if [ "$run_deny" = true ]; then
  # Asked through cargo, which also finds subcommands in ~/.cargo/bin when that is not on PATH.
  cargo deny --version >/dev/null 2>&1 || fail "cargo-deny is not installed (cargo install cargo-deny, or pass --no-deny)"
  cargo deny check
fi
