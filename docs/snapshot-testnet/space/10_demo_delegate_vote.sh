#!/usr/bin/env bash
# Gasless. The demo "delegate" (the throwaway) votes For with a reason on
# "Lower protocol fee to 0.2%". Idempotent: refuses if it already voted there.
# DRYRUN=1 signs but does not POST. Keeps no signature on disk (lib.sh scrub).
source "$(dirname "$0")/lib.sh"
S=$(space_id)
[ "$S" = "bloom-ballot-182c.eth" ] || { echo "refusing: only our own testnet space" >&2; exit 1; }
P=0x5fa4f371c47a9691128eaa0bc079abed859f73b9f2e0afb453ff5a7d5b752607
REASON="Lower fees bring volume"
Q="{ votes(first: 1, where: {proposal: \"$P\", voter: \"$ADDR\"}) { id voter choice vp created reason app } }"
[ "$(gql "$Q" | jq '.data.votes|length')" = 0 ] || { echo "already voted on $P" >&2; exit 3; }
echo "vp: $(gql "{ vp(voter: \"$ADDR\", space: \"$S\", proposal: \"$P\") { vp vp_state } }" | jq -c .data.vp)"
OUT="$STATE/demo-vote"
trap 'scrub "$OUT"' EXIT
echo '{"Vote":[{"name":"from","type":"string"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"proposal","type":"string"},{"name":"choice","type":"uint32"},{"name":"reason","type":"string"},{"name":"app","type":"string"},{"name":"metadata","type":"string"}]}' > "$OUT.types.json"
jq -nc --arg f "$ADDR" --arg s "$S" --argjson ts "$(date +%s)" --arg p "$P" --arg r "$REASON" '{from:$f,space:$s,timestamp:$ts,proposal:$p,choice:1,reason:$r,app:"bloom-ballot",metadata:"{}"}' > "$OUT.msg.json"
sign_and_send "$OUT.types.json" "$OUT.msg.json" Vote "$OUT" >/dev/null
scrub "$OUT"
if [ "${DRYRUN:-0}" = 1 ]; then
  echo "DRYRUN: signed, not POSTed. Message that would be sent:"
  jq -c . "$OUT.msg.json"
  exit 0
fi
jq -e .id "$OUT.response.json" >/dev/null || { echo "sequencer refused:" >&2; cat "$OUT.response.json" >&2; exit 1; }
jq -c '{id,ipfs,relayer:.relayer.address}' "$OUT.response.json"
for _ in $(seq 1 30); do
  R=$(gql "$Q")
  [ "$(echo "$R" | jq '.data.votes|length')" -gt 0 ] && { echo "$R" | jq -c '.data.votes[0]'; break; }
  sleep 3
done
[ "$(gql "$Q" | jq '.data.votes|length')" -gt 0 ] || { echo "vote not visible on hub yet" >&2; exit 1; }
gql "{ proposal(id: \"$P\") { state scores scores_total votes } }" | jq -c .data.proposal
