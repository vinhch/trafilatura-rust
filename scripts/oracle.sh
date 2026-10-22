#!/usr/bin/env bash
#
# Oracle: the value upstream Python trafilatura (at the pinned commit) returns for a snippet, for
# writing expected values of ported tests. Runs on Linux through uv, elsewhere in Docker, because the
# lxml wheels bundle libxml2 2.14.6 on Linux and 2.11.9 on Windows (see scripts/fixtures.sh).
set -euo pipefail

readonly script_name="oracle.sh"
# Same image and uv as scripts/fixtures.sh, the source of truth
readonly docker_image="python:3.13-slim@sha256:98ce6fb13b3803744fbca11434f00818f1e300e4e48a4f204e07b2c928b6218e"
readonly uv_version="0.11.14"
# Checks the pin the way parity.generate does, then puts upstream on sys.path; the snippet follows it
readonly prelude='import os, pathlib
from parity import import_path
from parity.generate import check_pin
_root = pathlib.Path(os.environ["TRAFILATURA_UPSTREAM"])
check_pin(_root)
import_path(_root)'

usage() {
  cat <<EOF
Usage: scripts/$script_name PYTHON_CODE

Runs PYTHON_CODE with \`python -c\` in tools/parity with upstream trafilatura importable, and prints its
stdout. The checkout must be at the commit pinned in Cargo.toml. Dependencies come from
tools/parity/uv.lock. Linux runs uv directly; Windows and macOS run Docker.

  PYTHON_CODE  e.g. 'from trafilatura.utils import trim; print(repr(trim(" a  b ")))'

Environment: TRAFILATURA_UPSTREAM is the upstream checkout. Default: ../trafilatura.
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
esac
[ "$#" -eq 1 ] || fail "expected one argument, the Python code (see --help)"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
upstream="${TRAFILATURA_UPSTREAM:-$repo_root/../trafilatura}"
[ -f "$upstream/trafilatura/utils.py" ] || fail "$upstream is not an upstream trafilatura checkout"
upstream="$(cd "$upstream" && pwd)"
# Through the environment, so quotes and newlines reach Python unchanged
export ORACLE_CODE="$prelude"$'\n'"$1"

cd "$repo_root"
if [ "$(uname -s)" = "Linux" ]; then
  TRAFILATURA_UPSTREAM="$upstream" exec uv run --frozen --no-dev --directory tools/parity python -c "$ORACLE_CODE"
fi

command -v docker >/dev/null || fail "docker is required on this platform"
# Git Bash on Windows needs host paths in Windows form and must not rewrite the container paths
host_path() {
  if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi
}
export MSYS_NO_PATHCONV=1
exec docker run --rm \
  -v "$(host_path "$repo_root"):/work:ro" \
  -v "$(host_path "$upstream"):/upstream:ro" \
  -e PYTHONDONTWRITEBYTECODE=1 -e PIP_DISABLE_PIP_VERSION_CHECK=1 -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy \
  -e TRAFILATURA_UPSTREAM=/upstream -e ORACLE_CODE \
  -w /work/tools/parity "$docker_image" \
  sh -c 'pip install --quiet --root-user-action=ignore "uv==$0" && uv sync --frozen --no-dev --quiet && exec uv run --frozen --no-dev python -c "$ORACLE_CODE"' \
  "$uv_version"
