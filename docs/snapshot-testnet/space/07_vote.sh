#!/usr/bin/env bash
# Gasless. ONE vote (choice 1 = "For") with the exact snapshot.js voteTypes (types.ts:204-215).
source "$(dirname "$0")/lib.sh"
S=$(space_id); P=$(jq -r .id "$STATE/proposal.response.json")
[ -s "$STATE/vote.response.json" ] && jq -e .id "$STATE/vote.response.json" >/dev/null && { echo "vote already cast: $(jq -r .id "$STATE/vote.response.json")"; exit 0; }
echo "vp: $(gql "{ vp(voter: \"$ADDR\", space: \"$S\", proposal: \"$P\") { vp vp_by_strategy vp_state } }" | tee "$STATE/vp.json")"
echo '{"Vote":[{"name":"from","type":"string"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"proposal","type":"string"},{"name":"choice","type":"uint32"},{"name":"reason","type":"string"},{"name":"app","type":"string"},{"name":"metadata","type":"string"}]}' > "$STATE/vote.types.json"
jq -nc --arg f "$ADDR" --arg s "$S" --argjson ts "$(date +%s)" --arg p "$P" '{from:$f,space:$s,timestamp:$ts,proposal:$p,choice:1,reason:"",app:"bloom-ballot",metadata:"{}"}' > "$STATE/vote.msg.json"
sign_and_send "$STATE/vote.types.json" "$STATE/vote.msg.json" Vote "$STATE/vote"
Q="{ votes(first: 10, where: {proposal: \"$P\", voter: \"$ADDR\"}) { id voter choice vp vp_state created reason app } }"
echo "hub query: $Q"
for i in $(seq 1 30); do R=$(gql "$Q"); [ "$(echo "$R" | jq '.data.votes|length')" -gt 0 ] && { echo "$R" | tee "$STATE/vote.hub.json" | jq -c .; break; }; sleep 3; done
gql "{ proposal(id: \"$P\") { scores scores_total votes } }" | tee "$STATE/proposal.after.json" | jq -c .
