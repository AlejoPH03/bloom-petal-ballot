#!/usr/bin/env bash
# NEEDS SEPOLIA ETH (1 tx). ENSv2 register, then checks the owner exactly as snapshot.js getEnsOwner does.
source "$(dirname "$0")/lib.sh"
L=$(label); SECRET=$(cat "$STATE/secret")
WAIT=$(( $(cat "$STATE/commit_ts") + 75 - $(date +%s) )); [ $WAIT -gt 0 ] && { echo "waiting ${WAIT}s for commitment age"; sleep $WAIT; }
ARGS=("$L" "$ADDR" "$SECRET" $ZERO_ADDR $ZERO_ADDR $DURATION $MOCK_USDC $ZERO32)
SIG='register(string,address,bytes32,address,address,uint64,address,bytes32)'
# dry-run first (free): subregistry=0/resolver=0 acceptance is UNVERIFIED until this passes
cast call --from "$ADDR" $ETH_REGISTRAR "$SIG(uint256)" "${ARGS[@]}" || { echo "register simulation reverted; not sending" >&2; exit 1; }
cast send --private-key "$(cat "$KEY_FILE")" $ETH_REGISTRAR "$SIG" "${ARGS[@]}" --json | jq '{tx:.transactionHash,status,gasUsed}'
OWNER=$(cast call $UNIVERSAL_HELPER 'findExactOwner(bytes)(address)' "$(dns_encode "$(space_id)")")
echo "findExactOwner($(space_id)) = $OWNER"; [ "$OWNER" = "$ADDR" ] && echo "OK: throwaway is the ENS owner -> Snapshot space controller"
