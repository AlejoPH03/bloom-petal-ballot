#!/usr/bin/env bash
# Developer cycle for a signing build of ballot on the local Bloom triad.
#
#   build -> check -> leak check -> enroll installer provenance -> (human restarts triad)
#   -> bloom petals build -> bloom petals install -> confirm hashes match
#
# Run it yourself from the repo root:  bash scripts/dev-cycle.sh
#
# Rules (CLAUDE.md, docs/plan-hello-signature.md, docs/plan-votes.md):
# - Provenance enrollment happens ONLY here. It rewrites the developer triad's
#   provenance-catalog.json, so it asks for confirmation first.
# - This script never restarts the triad: it pauses for you to do it.
# - It never writes /wallets/* (no policy.json; Machine proposes the policy
#   update itself on the first signing attempt), never completes or cancels a
#   ceremony, and never passes --force.
# - Do not change any file in the repo between enrollment and install: the
#   enrolled hash covers every file in the package directory.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

TRIAD_ENV="${BLOOM_TRIAD_ENV:-/tmp/bloom-triad-logs/triad.env}"
SIGN_INTENT="snapshot.vote"

die() { echo "dev-cycle: ERROR: $*" >&2; exit 1; }
step() { printf '\n==> %s\n' "$*"; }
need() { command -v "$1" >/dev/null 2>&1 || die "missing required command: $1"; }

load_triad_env() {
  [ -r "$TRIAD_ENV" ] || die "triad environment not found: $TRIAD_ENV (is the triad running?)"
  # shellcheck disable=SC1090
  source "$TRIAD_ENV"
  [ -n "${BLOOM_BIN:-}" ] && [ -x "$BLOOM_BIN" ] || die "BLOOM_BIN is not set to the triad-dev-harness bloom binary"
  [ -n "${BLOOM_PROVENANCE_CATALOG:-}" ] && [ -r "$BLOOM_PROVENANCE_CATALOG" ] \
    || die "BLOOM_PROVENANCE_CATALOG is not readable"
}

# Package hashes in the catalog whose Petal records authorize $SIGN_INTENT.
enrolled_hashes() {
  jq -r --arg class "$SIGN_INTENT" '
    .records[]
    | select(.subject.kind == "petal")
    | select(any(.operation_classes[]; .operation_class == $class))
    | .subject.package_hash' "$BLOOM_PROVENANCE_CATALOG" | sort -u
}

for tool in cargo petal wasm-tools jq; do need "$tool"; done
load_triad_env

step "1/8 host unit tests and route architecture check"
cargo test --manifest-path route/Cargo.toml
bash scripts/check-route-architecture.sh

step "2/8 petal build and petal check"
petal build --root .
petal check --root .

step "3/8 component imports (expect sign and vfs only on votes/new.json; store on votes/*; http on votes/new.json, votes/[vote_id].json and the proposal reads)"
while IFS= read -r wasm; do
  echo "== $wasm"
  wasm-tools component wit "$wasm" | grep -E 'import bloom:' || true
done < <(find petal/ballot -name '*.wasm' | sort)

step "4/8 leak check (no signature-sized hex outside the allowlist)"
bash scripts/leak-check.sh

step "5/8 enroll installer provenance (writes $BLOOM_PROVENANCE_CATALOG)"
config_dir="$(dirname "$BLOOM_PROVENANCE_CATALOG")"
before="$(enrolled_hashes)"
echo "Command:"
echo "  \"$BLOOM_BIN\" init triad-enroll-developer-petal-provenance \"$config_dir\" \"$ROOT\""
read -r -p "Type ENROLL to run it (anything else aborts): " answer
[ "$answer" = "ENROLL" ] || die "aborted before enrollment; nothing was changed"
"$BLOOM_BIN" init triad-enroll-developer-petal-provenance "$config_dir" "$ROOT"
after="$(enrolled_hashes)"
new_hashes="$(comm -13 <(printf '%s\n' "$before") <(printf '%s\n' "$after") | sed '/^$/d')"
case "$(printf '%s\n' "$new_hashes" | sed '/^$/d' | wc -l | tr -d ' ')" in
  1) enrolled_hash="$new_hashes" ;;
  0)
    # Re-enrolling an unchanged package keeps its existing record.
    [ -n "$after" ] || die "no catalog record authorizes $SIGN_INTENT after enrollment"
    enrolled_hash=""
    echo "No new $SIGN_INTENT record: this exact package was already enrolled." ;;
  *) die "enrollment added more than one $SIGN_INTENT package hash: $new_hashes" ;;
esac
[ -n "$enrolled_hash" ] && echo "Enrolled package hash: $enrolled_hash"

step "6/8 restart the triad (you do this; the script never does)"
cat <<'EOF'
Machine and Broker read the provenance catalog only at startup.
  1. In the launcher's terminal press Ctrl+C, then start the launcher again
     (docs/dev-setup-wsl.md).
  2. Wait until it reports the triad is ready.
  3. Do NOT change any file in this repository meanwhile.
Restarting invalidates any pending Petal ceremony; there should be none now.
EOF
read -r -p "Restart the triad, wait for ready, then press Enter... " _
load_triad_env   # the runtime directory changes on every launch

step "7/8 bloom petals build and install (never --force)"
build_out="$(bloom petals build .)"
printf '%s\n' "$build_out"
package_hash="$(printf '%s\n' "$build_out" | sed -n 's/^hash: \([0-9a-f]\{64\}\)$/\1/p' | head -n1)"
[ -n "$package_hash" ] || die "could not read the package hash from bloom petals build"
bloom petals install .

step "8/8 confirm the installed package is the enrolled package"
echo "bloom petals build hash: $package_hash"
if [ -n "$enrolled_hash" ] && [ "$enrolled_hash" != "$package_hash" ]; then
  die "MISMATCH: enrolled $enrolled_hash but built/installed $package_hash (a file changed after enrollment?)"
fi
enrolled_hashes | grep -qx "$package_hash" \
  || die "MISMATCH: $package_hash has no $SIGN_INTENT record in $BLOOM_PROVENANCE_CATALOG"
installed_line="$(bloom petals ls | grep -E 'app=petals/ballot/' || true)"
[ -n "$installed_line" ] || die "ballot is not listed by bloom petals ls after install"
installed_prefix="$(printf '%s\n' "$installed_line" | awk '{print $1}')"
case "$package_hash" in
  "$installed_prefix"*) ;;
  *) die "MISMATCH: bloom petals ls shows $installed_prefix for ballot, expected $package_hash" ;;
esac
echo "OK: enrolled == built == installed: $package_hash"

wallet="${BALLOT_WALLET:-main}"
votes="/petals/ballot/testnet/accounts/$wallet/0/votes"
cat <<EOF

Owner steps (docs/plan-votes.md section 12, "Live acceptance"; testnet only):
  bloom vfs cat $votes/new.json                      (schema, rules, last_write)
  bloom vfs write $votes/new.json --data '<body>'
    -> first: approval pending (<policy op id>). The owner opens the ceremony_url in
       /wallets/$wallet/policy-updates/pending/<id>/approval_challenge.json
       ("Approve policy change": adds $package_hash)
  bloom vfs write $votes/new.json --data '<body>'    (identical retry)
    -> approval pending (<request id>). Before approving, run the "verify" commands
       from $votes/<vote_id>.json and compare out.hex with the ceremony's exact hash.
       The owner opens the ceremony_url in /petal-signing-requests/<id>.json PROMPTLY.
  bloom vfs write $votes/new.json --data '<body>'    (immediately after approving)
    -> success; $votes/<vote_id>.json shows status "submitted", receipt.id = digest
  bloom vfs write $votes/new.json --data '<body>'    (identical retry)
    -> success; status "counted" once the hub shows the vote
If a vote expires (expired: approval_expired, proposal_closed, timestamp_stale),
write the same vote under a NEW vote_id.
EOF
