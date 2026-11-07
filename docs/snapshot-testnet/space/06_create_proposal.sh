#!/usr/bin/env bash
# Gasless. ONE proposal: single-choice, For/Against/Abstain, 7 days (snapshot.js proposalTypes, types.ts:140-158).
source "$(dirname "$0")/lib.sh"
S=$(space_id); NOW=$(date +%s); BLOCK=$(( $(cast block-number) - 5 ))
[ -s "$STATE/proposal.response.json" ] && jq -e .id "$STATE/proposal.response.json" >/dev/null && { echo "proposal already created: $(jq -r .id "$STATE/proposal.response.json")"; exit 0; }
echo '{"Proposal":[{"name":"from","type":"string"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"type","type":"string"},{"name":"title","type":"string"},{"name":"body","type":"string"},{"name":"discussion","type":"string"},{"name":"choices","type":"string[]"},{"name":"labels","type":"string[]"},{"name":"start","type":"uint64"},{"name":"end","type":"uint64"},{"name":"snapshot","type":"uint64"},{"name":"plugins","type":"string"},{"name":"privacy","type":"string"},{"name":"app","type":"string"}]}' > "$STATE/proposal.types.json"
jq -nc --arg f "$ADDR" --arg s "$S" --argjson ts $NOW --argjson b $BLOCK --argjson end $((NOW + 7*86400)) '{
  from:$f, space:$s, timestamp:$ts, type:"single-choice", title:"bloom-ballot test",
  body:"Throwaway test proposal for the Bloom ballot petal. Ignore.", discussion:"",
  choices:["For","Against","Abstain"], labels:[], start:$ts, end:$end, snapshot:$b,
  plugins:"{}", privacy:"", app:"bloom-ballot"}' > "$STATE/proposal.msg.json"
sign_and_send "$STATE/proposal.types.json" "$STATE/proposal.msg.json" Proposal "$STATE/proposal"
P=$(jq -r .id "$STATE/proposal.response.json"); echo "proposal id: $P"
for i in $(seq 1 20); do R=$(gql "{ proposal(id: \"$P\") { id space { id } author title choices type start end snapshot state scores scores_total } }"); echo "$R" | jq -e '.data.proposal' >/dev/null && { echo "$R" | tee "$STATE/proposal.hub.json" | jq -c .; exit 0; }; sleep 3; done
echo "proposal not visible on hub yet" >&2; exit 1
