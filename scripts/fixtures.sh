#!/usr/bin/env bash
#
# Generate the parity fixtures: upstream Python trafilatura at the pinned commit, run over its test
# corpus, written to .temp/fixtures/<pin>/. Fixtures are generated on Linux only (the lxml wheels
# bundle libxml2 2.14.6 there, 2.11.9 on Windows), so Windows and macOS use Docker.
set -euo pipefail

readonly script_name="fixtures.sh"
# python:3.13-slim
readonly docker_image="python:3.13-slim@sha256:98ce6fb13b3803744fbca11434f00818f1e300e4e48a4f204e07b2c928b6218e"
readonly uv_version="0.11.14"

usage() {
  cat <<EOF
Usage: scripts/$script_name [--upstream PATH] [--docker] [--only SUBSTRING]

Runs tools/parity (parity.generate) against the upstream checkout at PATH and writes
.temp/fixtures/<pin>/ (index.json, pages/*.json, f1.json). The checkout must be at the commit
pinned in Cargo.toml. Dependencies come from tools/parity/uv.lock.

  --upstream PATH  Upstream git checkout. Default: \$TRAFILATURA_UPSTREAM, else ../trafilatura.
  --docker         Run in Docker even on Linux. Without it, Linux uses uv directly.
  --only SUBSTRING Keep pages whose upstream path contains SUBSTRING (skips f1.json).
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
mkdir -p .temp/fixtures

only_args=()
[ -z "$only" ] || only_args=(--only "$only")

if [ "$use_docker" = false ] && [ "$(uname -s)" = "Linux" ]; then
  exec uv run --frozen --no-dev --project tools/parity python -m parity.generate \
    --upstream "$upstream" --out "$repo_root/.temp/fixtures" ${only_args[@]+"${only_args[@]}"}
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
  -e PYTHONDONTWRITEBYTECODE=1 -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy \
  -w /work/tools/parity "$docker_image" \
  sh -c 'pip install --quiet --root-user-action=ignore "uv==$0" && uv sync --frozen --no-dev --quiet && exec uv run --frozen --no-dev python -m parity.generate "$@"' \
  "$uv_version" --upstream /upstream --out /work/.temp/fixtures ${only_args[@]+"${only_args[@]}"}
