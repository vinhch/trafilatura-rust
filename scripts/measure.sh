#!/usr/bin/env bash
#
# Measure the parity corpus: time (5-run median) and heap per page. The Rust side covers decode, parse and
# extraction of every page plus the synthetic pages; the Python side times decode + parse and extraction.
# The Rust targets write .temp/measure/<version>-rust-time.json and <version>-rust-heap.json (and
# -synthetic.json files), the Python side <version>-python.json; --merge writes <version>-rust.json and the
# merged <version>.json and prints a Markdown table. Needs the fixtures (scripts/fixtures.sh): their
# index.json lists the pages. Run it by hand; the numbers depend on the machine. The Rust targets take about
# 5 minutes and need about 1 GB of heap for the synthetic pages.
set -euo pipefail

readonly script_name="measure.sh"
readonly docker_image="trafilatura-measure"
# Pages listed one per line in the table when --only is used; without it, totals only
readonly table_rows=15

usage() {
  cat <<EOF
Usage: scripts/$script_name [--upstream PATH] [--docker] [--only SUBSTRING]

Runs tests/measure_time.rs and tests/measure_heap.rs (release build, feature __internals; two binaries,
because the dhat allocator of the heap one would slow the timings), then tools/parity (parity.measure) against
the upstream checkout at PATH, then merges both into .temp/measure/<version>.json. The Python side runs
in the same place as the Rust side: natively on Linux, in one Linux container (scripts/measure.Dockerfile)
elsewhere, so both sides always share a system. Absolute numbers still depend on the machine.

  --upstream PATH  Upstream git checkout. Default: \$TRAFILATURA_UPSTREAM, else ../trafilatura.
  --docker         Run both sides in the container even on Linux.
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

# Off Linux the container runs this script again (as Linux, without --docker): one code path for both
if [ "$use_docker" = true ] || [ "$(uname -s)" != "Linux" ]; then
  command -v docker >/dev/null || fail "docker is required on this platform (or run on Linux)"
  # Git Bash on Windows needs host paths in Windows form and must not rewrite the container paths
  host_path() {
    if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi
  }
  export MSYS_NO_PATHCONV=1
  docker build -f "$(host_path "$repo_root/scripts/measure.Dockerfile")" -t "$docker_image" "$(host_path "$repo_root/scripts")" >&2
  # The target dir, cargo registry and uv cache live in named volumes: they survive --rm and keep a Linux
  # target dir out of the host checkout
  exec docker run --rm     -v "$(host_path "$repo_root"):/work"     -v "$(host_path "$upstream"):/upstream:ro"     -v trafilatura-measure-target:/work/target     -v trafilatura-measure-cargo:/usr/local/cargo/registry     -v trafilatura-measure-uv:/root/.cache/uv     -e TRAFILATURA_MEASURE_RUNTIME=docker -e TRAFILATURA_UPSTREAM=/upstream     -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy -e PYTHONDONTWRITEBYTECODE=1     -w /work "$docker_image"     scripts/measure.sh ${only_args[@]+"${only_args[@]}"}
fi

echo "== Rust: load_html and extract over the corpus (time, then heap) ==" >&2
TRAFILATURA_UPSTREAM="$upstream" TRAFILATURA_MEASURE=1 TRAFILATURA_MEASURE_ONLY="$only"   cargo test --locked --release --features __internals --test measure_time --test measure_heap -- --nocapture >&2

echo "== Python: upstream load_html and extract ==" >&2
uv run --frozen --no-dev --directory tools/parity python -m parity.measure   --upstream "$upstream" --out "$repo_root/.temp/measure" ${only_args[@]+"${only_args[@]}"}
exec uv run --frozen --no-dev --directory tools/parity python -m parity.measure   --merge --out "$repo_root/.temp/measure" --rows "${rows:-0}"
