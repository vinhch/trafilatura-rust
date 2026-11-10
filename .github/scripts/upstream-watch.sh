#!/usr/bin/env bash
#
# Open an issue when upstream Python trafilatura tags a release newer than the pin in Cargo.toml.
# Run by .github/workflows/upstream-watch.yml on ubuntu with gh and jq. CI only: it has no .ps1 twin,
# since the .sh/.ps1 pair rule covers scripts/.
#
# Arguments are passed to scripts/upstream.sh (--to REF, for testing). The upstream checkout comes from
# TRAFILATURA_UPSTREAM, gh reads GH_TOKEN and GH_REPO. The issue body goes through --body-file: a commit
# subject is data and never reaches a shell or a workflow expression.
set -euo pipefail

readonly script_name="upstream-watch.sh"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"
work="${RUNNER_TEMP:-${TMPDIR:-/tmp}}"

scripts/upstream.sh "$@" >"$work/upstream.txt"

# Anchored: a commit subject such as "keep the list up to date" must not match.
if grep -qx 'up to date' "$work/upstream.txt" || grep -Eq '^note: .* is older than the pin' "$work/upstream.txt"; then
  echo "$script_name: no newer upstream release"
  exit 0
fi

tag="$(awk '/^target:/ { print $2 }' "$work/upstream.txt")"
[ -n "$tag" ] || {
  echo "$script_name: no target tag in the output of scripts/upstream.sh" >&2
  exit 1
}
title="Upstream release $tag"

# All states: a closed issue still means the release was handled.
if gh issue list --state all --limit 500 --json title | jq -e --arg title "$title" 'any(.[]; .title == $title)' >/dev/null; then
  echo "$script_name: an issue titled '$title' exists"
  exit 0
fi

{
  echo "Upstream tagged $tag, newer than the release this port follows."
  echo
  # Indented, not fenced: a subject holding backticks cannot end the block.
  sed 's/^/    /' "$work/upstream.txt"
  echo
  echo "Next: follow docs/upstream-sync.md."
} >"$work/body.md"

gh issue create --title "$title" --body-file "$work/body.md"
