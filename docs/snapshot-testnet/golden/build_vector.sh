#!/usr/bin/env bash
# Builds the deterministic golden Vote vector (offline; nothing is submitted).
set -euo pipefail
cd "$(dirname "$0")"
FROM=${GOLDEN_FROM:-0x042abe4EE65cBC71Bd11297DCea2d69e1650a596}                 # checksummed, as snapshot.js sign() does
SPACE="bloom-ballot-test.eth"                    # synthetic
PROPOSAL=$(cast keccak "bloom-ballot golden vector")   # synthetic 32-byte id
TS=1793923200                                    # 2026-11-06T00:00:00Z, fixed
CHOICE=1; REASON=""; APP="bloom-ballot"; METADATA="{}"
# Exactly the snapshot.js voteTypes order (src/sign/types.ts:204-215)
TYPES='{"Vote":[{"name":"from","type":"string"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"proposal","type":"string"},{"name":"choice","type":"uint32"},{"name":"reason","type":"string"},{"name":"app","type":"string"},{"name":"metadata","type":"string"}]}'
jq -nc --arg from "$FROM" --arg space "$SPACE" --argjson ts $TS --arg p "$PROPOSAL" --argjson c $CHOICE \
   --arg r "$REASON" --arg a "$APP" --arg m "$METADATA" \
   '{from:$from,space:$space,timestamp:$ts,proposal:$p,choice:$c,reason:$r,app:$a,metadata:$m}' > message.json
# Envelope `data` (what goes to the sequencer: no EIP712Domain, no primaryType)
jq -nc --argjson types "$TYPES" --slurpfile msg message.json \
   '{domain:{name:"snapshot",version:"0.1.4"},types:$types,message:$msg[0]}' > envelope_data.json
# Full EIP-712 document for cast / Bloom (adds EIP712Domain + primaryType)
jq -c '.types = ({EIP712Domain:[{name:"name",type:"string"},{name:"version",type:"string"}]} + .types) | .primaryType="Vote"' \
   envelope_data.json > typed_data.json
echo "types sha256: $(printf %s "$(jq -c .types envelope_data.json)" | sha256sum | cut -d' ' -f1)"
