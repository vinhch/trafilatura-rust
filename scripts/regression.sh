#!/usr/bin/env bash
#
# Compare the sample's extraction heap and bench times of a base commit with the working tree, as the pull
# request job (bench.yml) does. The base is built in a detached worktree at .temp/regression/base-src, both
# sides run the heap sample (tests/measure_heap.rs, TRAFILATURA_MEASURE_ONLY=@sample) and the criterion bench
# (benches/extract.rs), and tools/parity (parity.measure --compare) prints the Markdown summary and applies
# the heap gate. Needs the fixtures (scripts/fixtures.sh) and the upstream checkout. Takes several minutes:
# four cargo runs, and the first build of the bench dependencies.
set -euo pipefail

readonly script_name="regression.sh"

usage() {
  cat <<EOF
Usage: scripts/$script_name [--base REF] [--upstream PATH]

Measures the working tree and REF with the same two commands, writes both into .temp/regression/{base,head}/,
then prints the comparison (also appended to \$GITHUB_STEP_SUMMARY when set). Exits 1 when the summed heap
numbers grow by more than the gate in tools/parity/parity/measure.py. When the measuring code (tests/measure,
tests/measure_heap.rs, tests/common, benches) or the upstream pin differs between REF and the working tree, the
two sides would not measure alike: only the head is measured and listed, and the exit code is 0.

  --base REF       Commit to compare with. Default: main.
  --upstream PATH  Upstream git checkout. Default: \$TRAFILATURA_UPSTREAM, else ../trafilatura.
  -h, --help       Show this message.
EOF
}

fail() {
  echo "$script_name: $*" >&2
  exit 1
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
upstream="${TRAFILATURA_UPSTREAM:-$repo_root/../trafilatura}"
base="main"
while [ "$#" -gt 0 ]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    --base)
      [ "$#" -ge 2 ] || fail "--base needs a value"
      base="$2"
      shift
      ;;
    --upstream)
      [ "$#" -ge 2 ] || fail "--upstream needs a value"
      upstream="$2"
      shift
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
  shift
done

cd "$repo_root"
[ -f "$upstream/trafilatura/utils.py" ] || fail "$upstream is not an upstream trafilatura checkout"
upstream="$(cd "$upstream" && pwd)"
git rev-parse --verify --quiet "$base^{commit}" >/dev/null || fail "$base is not a commit"

# The upstream table of Cargo.toml up to the next table header; CR stripped for a CRLF checkout.
pin_table() {
  tr -d '\r' | awk '/^\[/ { in_table = ($0 == "[package.metadata.upstream]") } in_table'
}

# The fixtures live under the pinned release, which is the same on both sides unless head-only.
release="$(pin_table <Cargo.toml | awk '$1 == "version" { gsub(/^[^=]*= *"|".*$/, ""); print; exit }')"
[ -n "$release" ] || fail "Cargo.toml has no version under [package.metadata.upstream]"
fixtures="$repo_root/.temp/fixtures/$release"
[ -f "$fixtures/index.json" ] || fail "no fixtures at $fixtures; run scripts/fixtures.sh"

base_src=".temp/regression/base-src"
# A run killed before its trap leaves the worktree registered; prune below drops it when its directory is
# already gone, so a failed remove is not an error.
if git worktree list --porcelain | grep -q '^worktree .*/\.temp/regression/base-src$'; then
  git worktree remove --force "$base_src" || true
fi
rm -rf .temp/regression
git worktree prune
mkdir -p .temp/regression/base .temp/regression/head

cleanup() {
  git worktree remove --force "$base_src" >/dev/null 2>&1 || true
  git worktree prune
}
trap cleanup EXIT

head_only=""
if ! git diff --quiet "$base" -- tests/measure tests/measure_heap.rs tests/common benches; then
  head_only="measuring code differs between base and head"
elif [ "$(git show "$base:Cargo.toml" | pin_table)" != "$(pin_table <Cargo.toml)" ]; then
  head_only="upstream pin differs between base and head"
fi

# Runs the heap sample and the bench in the tree at $1 and leaves heap-sample.json and criterion/ in $2.
# Both trees build into the head's target dir, so the dependencies compile once. Criterion writes
# <group>/<bench>/new/estimates.json under CRITERION_HOME instead of the target dir.
measure_tree() {
  local tree="$1" out="$2"
  (
    cd "$tree"
    export CARGO_TARGET_DIR="$repo_root/target" TRAFILATURA_UPSTREAM="$upstream" TRAFILATURA_FIXTURES="$fixtures"
    echo "== heap sample in $tree ==" >&2
    TRAFILATURA_MEASURE=1 TRAFILATURA_MEASURE_ONLY=@sample \
      cargo test --locked --release --features __internals --test measure_heap -- --nocapture >&2
    echo "== bench in $tree ==" >&2
    CRITERION_HOME="$out/criterion" \
      cargo bench --locked --features __internals --bench extract -- --noplot >&2
  )
  cp "$tree/.temp/measure/$release-rust-heap-sample.json" "$out/heap-sample.json"
}

if [ -z "$head_only" ]; then
  git worktree add --detach "$base_src" "$base" >&2
  measure_tree "$repo_root/$base_src" "$repo_root/.temp/regression/base"
fi
measure_tree "$repo_root" "$repo_root/.temp/regression/head"

head_only_args=()
[ -z "$head_only" ] || head_only_args=(--head-only "$head_only")
status=0
uv run --frozen --no-dev --directory tools/parity python -m parity.measure \
  --compare "$repo_root/.temp/regression" ${head_only_args[@]+"${head_only_args[@]}"} |
  tee -a "${GITHUB_STEP_SUMMARY:-/dev/null}" || status=$?
exit "$status"
