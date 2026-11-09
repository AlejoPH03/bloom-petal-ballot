#!/usr/bin/env bash
# Fails if signature-sized hex (a run of 130 or more hex digits: 65 bytes) is
# in a tracked file, artifacts/, petal/, or a file or directory passed as an
# argument. A run is allowed when removing the values in
# scripts/leak-allowlist.txt leaves only pieces shorter than that.
#
#   bash scripts/leak-check.sh [path ...]
#
# Exit 0 clean, 1 a leak, 2 misuse (bad allowlist entry, missing path).
# It reports file, length and a sha256 prefix: never the value itself.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export LC_ALL=C

MIN_RUN=130
ALLOWLIST="$ROOT/scripts/leak-allowlist.txt"

fail_usage() { echo "leak-check: $*" >&2; exit 2; }

[ -r "$ALLOWLIST" ] || fail_usage "cannot read $ALLOWLIST"
declare -A allowed=()
line_no=0
while IFS= read -r line || [ -n "$line" ]; do
  line_no=$((line_no + 1))
  case "$line" in ''|\#*) continue ;; esac
  # Entry: <hex run> # <why this public value is here>
  if ! [[ "$line" =~ ^([0-9a-fA-F]+)[[:space:]]+\#[[:space:]]*[^[:space:]] ]]; then
    fail_usage "leak-allowlist.txt:$line_no: expected '<hex> # <comment>'"
  fi
  value="${BASH_REMATCH[1]}"
  [ "${#value}" -ge "$MIN_RUN" ] || fail_usage "leak-allowlist.txt:$line_no: shorter than $MIN_RUN hex digits"
  allowed["${value,,}"]=1
done < "$ALLOWLIST"

files=()
while IFS= read -r -d '' path; do files+=("$ROOT/$path"); done < <(git -C "$ROOT" ls-files -z)
for dir in "$ROOT/artifacts" "$ROOT/petal"; do
  [ -d "$dir" ] || continue
  while IFS= read -r -d '' path; do files+=("$path"); done < <(find "$dir" -type f -print0)
done
for arg in "$@"; do
  if [ -f "$arg" ]; then
    files+=("$arg")
  elif [ -d "$arg" ]; then
    while IFS= read -r -d '' path; do files+=("$path"); done < <(find "$arg" -type f -print0)
  else
    fail_usage "no such file or directory: $arg"
  fi
done

# Compiled code puts arbitrary bytes next to an allowlisted table, so exact
# matching alone would break on rebuilds; a remainder of 130 digits still fails.
is_allowed() {
  local run="${1,,}" value part
  for value in "${!allowed[@]}"; do run="${run//$value/ }"; done
  [[ "$run" == *" "* ]] || return 1
  for part in $run; do [ "${#part}" -lt "$MIN_RUN" ] || return 1; done
}

leaks=0
scanned=0
for file in "${files[@]}"; do
  [ -f "$file" ] || continue   # tracked but deleted in the worktree
  scanned=$((scanned + 1))
  while IFS= read -r run; do
    is_allowed "$run" && continue
    fingerprint="$(printf %s "${run,,}" | sha256sum | cut -c1-16)"
    echo "leak-check: ${file#"$ROOT"/}: ${#run} hex digits, sha256 $fingerprint" >&2
    leaks=$((leaks + 1))
  done < <(grep -aoE "[0-9a-fA-F]{$MIN_RUN,}" -- "$file" || true)
done

if [ "$leaks" -ne 0 ]; then
  echo "leak-check: FAILED: $leaks run(s) not in scripts/leak-allowlist.txt" >&2
  exit 1
fi
echo "leak-check: OK ($scanned files, ${#allowed[@]} allowlisted values)"
