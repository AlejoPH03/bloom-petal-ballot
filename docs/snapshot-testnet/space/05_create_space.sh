#!/usr/bin/env bash
# Gasless. Signed "settings" message by the ENS controller (sequencer writer/settings.ts:42-65).
source "$(dirname "$0")/lib.sh"
S=$(space_id)
SETTINGS=$(jq -nc --arg a "$ADDR" '{
  name:"bloom-ballot test", about:"Throwaway sandbox for Bloom ballot petal integration tests. Not a real DAO.",
  network:"11155111", symbol:"VOTE", private:true,
  strategies:[{name:"ticket", network:"11155111", params:{symbol:"VOTE", value:1}}],
  admins:[$a], members:[], moderators:[],
  filters:{minScore:0, onlyMembers:true},           # only admins/members/moderators may propose (writer/proposal.ts:152-160)
  validation:{name:"any", params:{}},
  voteValidation:{name:"any", params:{}},           # any address; vp>0 still enforced (writer/vote.ts:89)
  voting:{type:"single-choice", privacy:"", hideAbstain:false},
  categories:[], plugins:{}, treasuries:[], labels:[]
}')
echo '{"Space":[{"name":"from","type":"address"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"settings","type":"string"}]}' > "$STATE/space.types.json"
jq -nc --arg f "$ADDR" --arg s "$S" --argjson ts "$(date +%s)" --arg set "$SETTINGS" '{from:$f,space:$s,timestamp:$ts,settings:$set}' > "$STATE/space.msg.json"
sign_and_send "$STATE/space.types.json" "$STATE/space.msg.json" Space "$STATE/space"
for i in $(seq 1 20); do R=$(gql "{ space(id: \"$S\") { id admins strategies { name params } validation { name } voteValidation { name } filters { onlyMembers } voting { type } } }"); echo "$R" | jq -e '.data.space' >/dev/null && { echo "$R" | tee "$STATE/space.hub.json" | jq -c .; exit 0; }; sleep 3; done
echo "space not visible on hub yet" >&2; exit 1
