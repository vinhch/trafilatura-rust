#!/usr/bin/env bash
#
# Check the crate version in Cargo.toml: that it is a release version, and with --tag that it is the
# version the release tag names. release.yml calls it so a tag cannot publish a different version.
set -euo pipefail

readonly script_name="version.sh"

usage() {
  cat <<EOF
Usage: scripts/$script_name [--check] [--tag vX.Y.Z]

Prints the crate version from Cargo.toml.

  --check        Fail unless the version is X.Y.Z or X.Y.Z-pre (semver, no build metadata).
  --tag TAG      Fail unless TAG is "v" followed by the version. Implies --check.
  -h, --help     Show this message.
EOF
}

fail() {
  echo "$script_name: $*" >&2
  exit 1
}

check=false
tag=""
while [ "$#" -gt 0 ]; do
  case "$1" in
    -h | --help)
      usage
      exit 0
      ;;
    --check)
      check=true
      ;;
    --tag)
      [ "$#" -ge 2 ] || fail "--tag needs a value"
      tag="$2"
      check=true
      shift
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
  shift
done

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
manifest="$repo_root/Cargo.toml"

# The first `version` key after [package], before the next table header.
version="$(awk '
  /^\[/ { in_package = ($0 == "[package]") }
  in_package && /^version *=/ { gsub(/^version *= *"|".*$/, ""); print; exit }
' "$manifest")"
[ -n "$version" ] || fail "no version under [package] in $manifest"

if [ "$check" = true ]; then
  [[ "$version" =~ ^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$ ]] ||
    fail "version '$version' is not X.Y.Z or X.Y.Z-pre"
  if [ -n "$tag" ] && [ "$tag" != "v$version" ]; then
    fail "tag '$tag' does not match Cargo.toml version '$version' (expected 'v$version')"
  fi
fi

echo "$version"
