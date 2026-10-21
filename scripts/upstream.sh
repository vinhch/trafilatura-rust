#!/usr/bin/env bash
#
# Show what changed in upstream Python trafilatura since the release this port follows, as the first
# step of the sync procedure in AGENTS.md. Read-only: it changes neither checkout.
set -euo pipefail

readonly script_name="upstream.sh"

usage() {
  cat <<EOF
Usage: scripts/$script_name [--upstream PATH] [--to REF] [--patch]

Reads the pinned upstream version and commit from Cargo.toml [package.metadata.upstream], then
prints, for the upstream checkout at PATH, the commits and the per-file change summary between the
pin and REF under trafilatura/ and tests/.

  --upstream PATH  Upstream git checkout. Default: \$TRAFILATURA_UPSTREAM, else ../trafilatura.
  --to REF         Target ref. Default: the highest v* tag in the checkout.
  --patch          Print the full diff of trafilatura/ instead of the summary.
  -h, --help       Show this message.

Fetch tags in the upstream checkout first (git -C PATH fetch --tags); this script never fetches.
EOF
}

fail() {
  echo "$script_name: $*" >&2
  exit 1
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
upstream="${TRAFILATURA_UPSTREAM:-$repo_root/../trafilatura}"
target=""
patch=false
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
    --to)
      [ "$#" -ge 2 ] || fail "--to needs a value"
      target="$2"
      shift
      ;;
    --patch)
      patch=true
      ;;
    *)
      fail "unknown argument: $1"
      ;;
  esac
  shift
done

# A key of [package.metadata.upstream], before the next table header.
pin_value() {
  awk -v key="$1" '
    /^\[/ { in_table = ($0 == "[package.metadata.upstream]") }
    in_table && $1 == key { gsub(/^[^=]*= *"|".*$/, ""); print; exit }
  ' "$repo_root/Cargo.toml"
}

pinned_version="$(pin_value version)"
pinned_commit="$(pin_value commit)"
[ -n "$pinned_version" ] && [ -n "$pinned_commit" ] ||
  fail "Cargo.toml has no version and commit under [package.metadata.upstream]"

git -C "$upstream" rev-parse --git-dir >/dev/null 2>&1 || fail "$upstream is not a git checkout"
git -C "$upstream" cat-file -e "$pinned_commit^{commit}" 2>/dev/null ||
  fail "pinned commit $pinned_commit is not in $upstream; fetch it first"

if [ -z "$target" ]; then
  target="$(git -C "$upstream" tag --list 'v*' --sort=-v:refname | head -n 1)"
  [ -n "$target" ] || fail "no v* tag in $upstream; fetch tags or pass --to"
fi
target_commit="$(git -C "$upstream" rev-parse --verify "$target^{commit}" 2>/dev/null)" ||
  fail "ref '$target' does not resolve in $upstream"

echo "pinned:  v$pinned_version  $pinned_commit"
echo "target:  $target  $target_commit"
if [ "$pinned_commit" = "$target_commit" ]; then
  echo "up to date"
  exit 0
fi
if git -C "$upstream" merge-base --is-ancestor "$target_commit" "$pinned_commit"; then
  echo "note: $target is older than the pin; the summary shows changes in reverse"
fi

if [ "$patch" = true ]; then
  git -C "$upstream" --no-pager diff "$pinned_commit" "$target_commit" -- trafilatura/
  exit 0
fi

echo
echo "commits:"
git -C "$upstream" --no-pager log --oneline --no-merges "$pinned_commit..$target_commit" -- trafilatura/ tests/
echo
echo "files:"
git -C "$upstream" --no-pager diff --stat=120 "$pinned_commit" "$target_commit" -- trafilatura/ tests/
