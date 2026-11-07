#!/usr/bin/env bash
# NEEDS SEPOLIA ETH (1 tx). ENSv2 commit (docs.ens.domains/ensv2/eth-registrar).
source "$(dirname "$0")/lib.sh"
L=$(label)
[ -s "$STATE/secret" ] || { (umask 077; cast keccak "$(head -c32 /dev/urandom | od -An -tx1 | tr -d ' \n')" > "$STATE/secret"); }
SECRET=$(cat "$STATE/secret")
C=$(cast call $ETH_REGISTRAR 'makeCommitment(string,address,bytes32,address,address,uint64,bytes32)(bytes32)' \
     "$L" "$ADDR" "$SECRET" $ZERO_ADDR $ZERO_ADDR $DURATION $ZERO32)
echo "$C" > "$STATE/commitment"
cast send --private-key "$(cat "$KEY_FILE")" $ETH_REGISTRAR 'commit(bytes32)' "$C" --json | jq '{tx:.transactionHash,status}'
date +%s > "$STATE/commit_ts"
echo "committed $C; wait >= MIN_COMMITMENT_AGE (60s), < 24h, then run 04_register.sh"
