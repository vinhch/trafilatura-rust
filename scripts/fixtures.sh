#!/usr/bin/env bash
#
# Parity fixtures: upstream Python trafilatura at the pinned commit, run over its test corpus. They
# live in git as one archive, tools/parity/fixtures/fixtures-<version>.tar.zst, and are unpacked into
# .temp/fixtures/<version>/. Generating them (--generate) is slow and runs on Linux only (the lxml wheels
# bundle libxml2 2.14.6 there, 2.11.9 on Windows), so Windows and macOS use Docker.
set -euo pipefail

readonly script_name="fixtures.sh"
# python:3.13-slim
readonly docker_image="python:3.13-slim@sha256:98ce6fb13b3803744fbca11434f00818f1e300e4e48a4f204e07b2c928b6218e"
readonly uv_version="0.11.14"

usage() {
  cat <<EOF
Usage: scripts/$script_name
       scripts/$script_name [--generate] [--pack] [--upstream PATH] [--docker] [--only SUBSTRING]

Without flags: unpacks tools/parity/fixtures/fixtures-<version>.tar.zst into .temp/fixtures/<version>/, or does
nothing when that directory exists (delete it to unpack again). Needs no Python or Docker, only a tar
that reads zstd: \`tar --zstd\` when zstd is on PATH, else bsdtar (on Windows
%SystemRoot%\\System32\\tar.exe). Otherwise install zstd.

  --generate       Run tools/parity (parity.generate) against the upstream checkout and write
                   .temp/fixtures/<version>/ (index.json, pages/*.json, f1.json). The checkout must be at the
                   commit pinned in Cargo.toml. Dependencies come from tools/parity/uv.lock.
  --pack           Write the archive of .temp/fixtures/<version>/ (parity.pack), after --generate when both are
                   given. An existing archive is kept: delete it to pack again. After a new upstream release or a
                   generator change delete the archive and .temp/fixtures/<version>, then run
                   --generate --pack.
  --upstream PATH  Upstream git checkout. Default: \$TRAFILATURA_UPSTREAM, else ../trafilatura.
  --docker         Run --generate and --pack in Docker even on Linux. Without it, Linux uses uv directly.
  --only SUBSTRING Keep pages whose upstream path contains SUBSTRING (skips f1.json); implies --generate.
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
generate=false
pack=false
only=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    --generate)
      generate=true
      ;;
    --pack)
      pack=true
      ;;
    --upstream)
      [ "$#" -ge 2 ] || fail "--upstream needs a value"
      upstream="$2"
      generate=true
      shift
      ;;
    --docker)
      use_docker=true
      ;;
    --only)
      [ "$#" -ge 2 ] || fail "--only needs a value"
      only="$2"
      generate=true
      shift
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
  shift
done

cd "$repo_root"
# Only the `version` key of [package.metadata.upstream]: the package's own `version` comes earlier
version="$(awk '/^\[/ { in_table = ($0 == "[package.metadata.upstream]") } in_table && /^version = / { gsub(/^version = "|"\r?$/, ""); print; exit }' Cargo.toml)"
[ -n "$version" ] || fail "no [package.metadata.upstream] version in Cargo.toml"
archive="tools/parity/fixtures/fixtures-$version.tar.zst"
fixtures_dir=".temp/fixtures"

unpack() {
  if [ -d "$fixtures_dir/$version" ]; then
    echo "fixtures present, delete .temp/fixtures/$version to unpack again"
    return 0
  fi
  [ -f "$archive" ] || fail "no archive for version $version at $archive; run scripts/$script_name --generate --pack"
  local tar_cmd
  if command -v zstd >/dev/null; then
    tar_cmd=(tar --zstd -xf)
  elif tar --version 2>&1 | grep -qi bsdtar; then
    tar_cmd=(tar -xf)
  elif [ -n "${SYSTEMROOT:-}" ] && windows_tar="$(cygpath -u "$SYSTEMROOT" 2>/dev/null || echo "$SYSTEMROOT")/System32/tar.exe" && [ -x "$windows_tar" ]; then
    tar_cmd=("$windows_tar" -xf)
  else
    fail "no tar that reads zstd: install zstd, or bsdtar"
  fi
  # Extract beside the target and rename, so an interrupted run never leaves a half-filled directory
  local staging="$fixtures_dir/$version.tmp"
  rm -rf "$staging"
  mkdir -p "$staging"
  "${tar_cmd[@]}" "$archive" -C "$staging" --strip-components=1
  mv "$staging" "$fixtures_dir/$version"
  echo "unpacked $archive into $fixtures_dir/$version"
}

if [ "$generate" = false ] && [ "$pack" = false ]; then
  unpack
  exit 0
fi

[ "$pack" = false ] || [ "$only" = "" ] || fail "--pack needs a full set: it cannot be combined with --only"
if [ "$generate" = true ] && [ "$pack" = true ] && [ -f "$archive" ]; then
  echo "archive exists, delete it to pack again: $archive"
  exit 0
fi
if [ "$generate" = true ]; then
  [ -f "$upstream/trafilatura/utils.py" ] || fail "$upstream is not an upstream trafilatura checkout"
  upstream="$(cd "$upstream" && pwd)"
fi
mkdir -p "$fixtures_dir" tools/parity/fixtures

only_args=()
[ -z "$only" ] || only_args=(--only "$only")

if [ "$use_docker" = false ] && [ "$(uname -s)" = "Linux" ]; then
  run_parity() {
    uv run --frozen --no-dev --directory tools/parity python -m "$@"
  }
  if [ "$generate" = true ]; then
    run_parity parity.generate --upstream "$upstream" --out "$repo_root/$fixtures_dir" ${only_args[@]+"${only_args[@]}"}
  fi
  if [ "$pack" = true ]; then
    run_parity parity.pack --fixtures "$repo_root/$fixtures_dir" --out "$repo_root/tools/parity/fixtures"
  fi
  exit 0
fi

command -v docker >/dev/null || fail "docker is required on this platform (or pass --docker on Linux)"
# Git Bash on Windows needs host paths in Windows form and must not rewrite the container paths
host_path() {
  if command -v cygpath >/dev/null; then cygpath -w "$1"; else echo "$1"; fi
}
export MSYS_NO_PATHCONV=1
mounts=(
  -v "$(host_path "$repo_root"):/work:ro"
  -v "$(host_path "$repo_root/.temp"):/work/.temp"
  -v "$(host_path "$repo_root/tools/parity/fixtures"):/work/tools/parity/fixtures"
)
generate_args=()
if [ "$generate" = true ]; then
  mounts+=(-v "$(host_path "$upstream"):/upstream:ro")
  generate_args=(--upstream /upstream --out /work/.temp/fixtures ${only_args[@]+"${only_args[@]}"})
fi
# $0 = uv version, $1 and $2 = run generate and pack (1 or 0), the rest goes to parity.generate
inner='pip install --quiet --root-user-action=ignore "uv==$0" && uv sync --frozen --no-dev --quiet && gen=$1 && pack=$2 && shift 2 &&
  { [ "$gen" = 0 ] || uv run --frozen --no-dev python -m parity.generate "$@"; } &&
  { [ "$pack" = 0 ] || uv run --frozen --no-dev python -m parity.pack --fixtures /work/.temp/fixtures --out /work/tools/parity/fixtures; }'
exec docker run --rm "${mounts[@]}" \
  -e PYTHONDONTWRITEBYTECODE=1 -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy \
  -w /work/tools/parity "$docker_image" \
  sh -c "$inner" "$uv_version" "$([ "$generate" = true ] && echo 1 || echo 0)" "$([ "$pack" = true ] && echo 1 || echo 0)" \
  ${generate_args[@]+"${generate_args[@]}"}
