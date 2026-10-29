#!/usr/bin/env bash
#
# Fuzz one target with libFuzzer, minimize one crash artifact, or minimize a corpus. Runs on Linux
# x86_64 with cargo-fuzz; scripts/fuzz.ps1 runs this script in a container.
set -euo pipefail

readonly script_name="fuzz.sh"
readonly fuzz_target_triple="x86_64-unknown-linux-gnuasan"
# libFuzzer limits, shared by a run, --tmin and --cmin so a minimized input reproduces under the same limits.
readonly timeout_seconds=37
readonly rss_limit_mb=6144
readonly max_len=1048576

usage() {
  cat <<EOF
Usage: scripts/$script_name [-h] <target> [seconds]
       scripts/$script_name --tmin <target> <artifact>
       scripts/$script_name --cmin <target>

Fuzzes <target> (parse_document or extract) for [seconds] (a positive integer, default 60), with the
corpus in fuzz/corpus/<target> and the upstream test pages as seeds. --tmin minimizes <artifact>, a
crash file, with the same limits. Needs cargo-fuzz and the $fuzz_target_triple target (Linux x86_64
only); the build is AddressSanitizer on the stable toolchain.

  -max_total_time=<seconds>  libFuzzer stops after this many seconds.
  -max_len=$max_len     the largest input libFuzzer generates, in bytes (1 MiB).
  -timeout=$timeout_seconds          one input running longer than this many seconds is a finding. 5 times the slowest
                             1 MiB linear page (7.3 s, extract under AddressSanitizer), rounded up.
  -rss_limit_mb=$rss_limit_mb    memory above this many MiB is a finding. 2 times the peak of a linear page at the
                             64 MiB input cap (2.9 GiB, extract), rounded up to a multiple of 1024.
  --tmin       minimize <artifact> instead of fuzzing.
  --cmin       minimize the corpus in fuzz/corpus/<target> in place, keeping the inputs that add coverage.
  -h, --help   Show this message.

Seed directories: tests/cache, tests/eval and tests/resources under \$TRAFILATURA_UPSTREAM, else
../trafilatura; each is used only if it exists.
EOF
}

fail() {
  usage >&2
  echo "$script_name: $*" >&2
  exit 1
}

check_target() {
  case "$1" in
    parse_document | extract) ;;
    *) fail "unknown or missing target: ${1:-}" ;;
  esac
}

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
limit_args=(-timeout="$timeout_seconds" -rss_limit_mb="$rss_limit_mb")

case "${1:-}" in
  -h | --help)
    usage
    exit 0
    ;;
  --tmin)
    [ "$#" -eq 3 ] || fail "--tmin takes <target> <artifact>"
    check_target "$2"
    [ -f "$3" ] || fail "artifact not found: $3"
    artifact="$(realpath "$3")"
    cd "$repo_root"
    exec cargo fuzz tmin --fuzz-dir fuzz --sanitizer none --target "$fuzz_target_triple" "$2" "$artifact" -- "${limit_args[@]}"
    ;;
  --cmin)
    [ "$#" -eq 2 ] || fail "--cmin takes <target>"
    check_target "$2"
    cd "$repo_root"
    corpus="fuzz/corpus/$2"
    before="$(find "$corpus" -type f 2> /dev/null | wc -l || true)"
    if [ "$before" -eq 0 ]; then
      echo "$script_name: no corpus in $corpus, nothing to minimize"
      exit 0
    fi
    # No -max_len and no seed dirs: cmin takes one corpus dir and rewrites it in place.
    cargo fuzz cmin --fuzz-dir fuzz --sanitizer none --target "$fuzz_target_triple" "$2" "$corpus" -- "${limit_args[@]}"
    echo "$script_name: $corpus held $before files, now $(find "$corpus" -type f | wc -l)"
    exit 0
    ;;
esac

[ "$#" -ge 1 ] && [ "$#" -le 2 ] || fail "expected <target> [seconds]"
target="$1"
seconds="${2:-60}"
check_target "$target"
[[ "$seconds" =~ ^[1-9][0-9]*$ ]] || fail "seconds must be a positive integer: $seconds"

cd "$repo_root"
corpus="fuzz/corpus/$target"
mkdir -p "$corpus"

upstream="${TRAFILATURA_UPSTREAM:-../trafilatura}"
seed_dirs=()
for sub in tests/cache tests/eval tests/resources; do
  if [ -d "$upstream/$sub" ]; then
    seed_dirs+=("$upstream/$sub")
  fi
done
if [ "${#seed_dirs[@]}" -gt 0 ]; then
  echo "$script_name: seed dirs: ${seed_dirs[*]}"
else
  echo "$script_name: no seed dirs"
fi

# ${seed_dirs[@]+...}: an empty array is an unbound variable under set -u on bash before 4.4.
exec cargo fuzz run --fuzz-dir fuzz --sanitizer none --target "$fuzz_target_triple" "$target" "$corpus" \
  ${seed_dirs[@]+"${seed_dirs[@]}"} -- -max_total_time="$seconds" -max_len="$max_len" "${limit_args[@]}"
