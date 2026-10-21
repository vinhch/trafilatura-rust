#!/usr/bin/env bash
#
# Measure decode + parse, Rust against upstream Python: time (5-run median) and heap per page over
# the parity corpus. Writes .temp/measure/<pin>-rust.json, <pin>-python.json and the merged
# <pin>.json, and prints a Markdown table. Needs the fixtures (scripts/fixtures.sh): their index.json
# lists the pages. Run it by hand; the numbers depend on the machine.
set -euo pipefail

readonly script_name="measure.sh"
# Same image and uv as scripts/fixtures.sh
readonly docker_image="python:3.13-slim@sha256:98ce6fb13b3803744fbca11434f00818f1e300e4e48a4f204e07b2c928b6218e"
readonly uv_version="0.11.14"
# Pages listed one per line in the table when --only is used; without it, totals only
readonly table_rows=15

usage() {
  cat <<EOF
Usage: scripts/$script_name [--upstream PATH] [--docker] [--only SUBSTRING]

Runs tests/measure_time.rs and tests/measure_heap.rs (release build, feature __internals; two binaries,
because the dhat allocator of the heap one would slow the timings), then tools/parity (parity.measure) against
the upstream checkout at PATH, then merges both into .temp/measure/<pin>.json. The Python side runs
where scripts/fixtures.sh runs: uv on Linux, Docker elsewhere. On Windows and macOS the two sides
therefore run on different systems (host and container): compare ratios between runs, not absolutes.

  --upstream PATH  Upstream git checkout. Default: \$TRAFILATURA_UPSTREAM, else ../trafilatura.
  --docker         Run the Python side in Docker even on Linux.
  --only SUBSTRING Keep pages whose upstream path contains SUBSTRING.
  -h, --help       Show this message.
EOF
}

fail() {
  echo "$script_name: $*" >&2
  exit 1
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
upstream="${TRAFILATURA_UPSTREAM:-$repo_root/../trafilatura}"
use_docker=false
only=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    --upstream)
      [ "$#" -ge 2 ] || fail "--upstream needs a value"
      upstream="$2"
      shift
      ;;
    --docker)
      use_docker=true
      ;;
    --only)
      [ "$#" -ge 2 ] || fail "--only needs a value"
      only="$2"
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
mkdir -p .temp/measure

only_args=()
rows=""
if [ -n "$only" ]; then
  only_args=(--only "$only")
  rows="$table_rows"
fi

echo "== Rust: load_html over the corpus (time, then heap) ==" >&2
TRAFILATURA_UPSTREAM="$upstream" TRAFILATURA_MEASURE=1 TRAFILATURA_MEASURE_ONLY="$only" \
  cargo test --release --features __internals --test measure_time --test measure_heap -- --nocapture >&2

echo "== Python: upstream load_html ==" >&2
if [ "$use_docker" = false ] && [ "$(uname -s)" = "Linux" ]; then
  uv run --frozen --no-dev --project tools/parity python -m parity.measure \
    --upstream "$upstream" --out "$repo_root/.temp/measure" ${only_args[@]+"${only_args[@]}"}
  exec uv run --frozen --no-dev --project tools/parity python -m parity.measure \
    --merge --out "$repo_root/.temp/measure" --rows "${rows:-0}"
fi

command -v docker >/dev/null || fail "docker is required on this platform (or pass --docker on Linux)"
# Git Bash on Windows needs host paths in Windows form and must not rewrite the container paths
host_path() {
  if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi
}
export MSYS_NO_PATHCONV=1
exec docker run --rm \
  -v "$(host_path "$repo_root"):/work:ro" \
  -v "$(host_path "$upstream"):/upstream:ro" \
  -v "$(host_path "$repo_root/.temp"):/work/.temp" \
  -e PYTHONDONTWRITEBYTECODE=1 -e TRAFILATURA_MEASURE_RUNTIME=docker -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy \
  -w /work/tools/parity "$docker_image" \
  sh -c 'rows=$1 && shift && pip install --quiet --root-user-action=ignore "uv==$0" && uv sync --frozen --no-dev --quiet && uv run --frozen --no-dev python -m parity.measure --upstream /upstream --out /work/.temp/measure "$@" && exec uv run --frozen --no-dev python -m parity.measure --merge --out /work/.temp/measure --rows "$rows"' \
  "$uv_version" "${rows:-0}" ${only_args[@]+"${only_args[@]}"}
