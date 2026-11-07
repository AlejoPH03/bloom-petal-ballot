#!/usr/bin/env bash
# Shared config for the step0 space scripts. Testnet ONLY.
set -euo pipefail
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(dirname "$HERE")"
STATE="$HERE/state"; mkdir -p "$STATE"; chmod 700 "$STATE"
export ETH_RPC_URL="${ETH_RPC_URL:-https://ethereum-sepolia-rpc.publicnode.com}"
SEQ="https://testnet.seq.snapshot.org/"          # snapshot.js constants.json:8
HUB="https://testnet.hub.snapshot.org/graphql"   # snapshot.js constants.json:7
# Path to a file holding a THROWAWAY private key (hex). Never use a real wallet key here.
KEY_FILE="${SNAPSHOT_KEY_FILE:?set SNAPSHOT_KEY_FILE to a file containing a THROWAWAY private key (chmod 600)}"
[ -r "$KEY_FILE" ] || { echo "cannot read SNAPSHOT_KEY_FILE" >&2; exit 1; }
ADDR="$(cast wallet address --private-key "$(cat "$KEY_FILE")")"
# ENSv2 Sepolia (docs.ens.domains/learn/deployments, verified on-chain 2026-11-06)
ETH_REGISTRAR=0xf633e7fc17e2bbe0d0965d18ec1821dcb754a3d3
MOCK_USDC=0x240b0316df57887dbbe58b586508b19e633a14aa      # accepted by getRegisterPrice; mint(address,uint256) is open
UNIVERSAL_HELPER=0xd453e5Bdb62CC3beA84341B1e306319C8Ffd7DFe # snapshot.js networks.json 11155111.ensUniversalHelper
ZERO_ADDR=0x0000000000000000000000000000000000000000
ZERO32=0x0000000000000000000000000000000000000000000000000000000000000000
DURATION=7776000   # 90 days (MIN_REGISTER_DURATION on-chain = 2419200 = 28 days)

[ "$(cast chain-id)" = "11155111" ] || { echo "RPC is not Sepolia" >&2; exit 1; }
case "$SEQ" in https://testnet.seq.snapshot.org/) ;; *) echo "refusing non-testnet sequencer" >&2; exit 1;; esac

label() { [ -n "${SPACE_LABEL:-}" ] && { echo "$SPACE_LABEL"; return; }; [ -s "$STATE/label" ] || echo "bloom-ballot-$(head -c2 /dev/urandom | od -An -tx1 | tr -d ' \n')" > "$STATE/label"; cat "$STATE/label"; }
space_id() { echo "$(label).eth"; }
gql() { jq -n --arg q "$1" '{query:$q}' | curl -s -m 30 -X POST "$HUB" -H 'content-type: application/json' -d @-; }
dns_encode() { python3 -I -c "import sys;print('0x'+b''.join(bytes([len(l)])+l.encode() for l in sys.argv[1].split('.')).hex()+'00')" "$1"; }

# sign_and_send <types.json (no EIP712Domain)> <message.json> <primaryType> <outprefix>
# Mirrors snapshot.js Client.sign()/send() (src/sign/index.ts:71-115).
sign_and_send() {
  local types="$1" msg="$2" ptype="$3" out="$4"
  jq -nc --slurpfile t "$types" --slurpfile m "$msg" '{domain:{name:"snapshot",version:"0.1.4"},types:$t[0],message:$m[0]}' > "$out.data.json"
  # sequencer matches sha256(JSON.stringify(types)) against hashedTypes.json (ingestor.ts:79-80)
  local th; th=$(printf %s "$(jq -c .types "$out.data.json")" | sha256sum | cut -d' ' -f1)
  # sha256 of the types we emit, as found in snapshot.js src/sign/hashedTypes.json (settings, proposal, vote)
  local known="110b37d3e2625cd6e1ec7b921e77773083b2a2949e69bd3a35fd9f1031ca5666 e037b37311c75c995f732b57cd456bdc685e5eac29668698be34388cafb14097 fb8fa9816cd42974e7f1af671aa548c8c458553364ed809e45042f141de8c0d5"
  if [ -n "${SNAPSHOT_JS_DIR:-}" ]; then grep -q "\"$th\"" "$SNAPSHOT_JS_DIR/src/sign/hashedTypes.json" || { echo "types hash $th not in hashedTypes.json" >&2; return 1; }
  else case " $known " in *" $th "*) ;; *) echo "types hash $th not in known list" >&2; return 1;; esac; fi
  jq -c --arg p "$ptype" '.types = ({EIP712Domain:[{name:"name",type:"string"},{name:"version",type:"string"}]} + .types) | .primaryType=$p' "$out.data.json" > "$out.typed.json"
  local sig; sig=$(cast wallet sign --private-key "$(cat "$KEY_FILE")" --data --from-file "$out.typed.json")
  cast wallet verify --address "$ADDR" --data --from-file "$out.typed.json" "$sig" >/dev/null
  jq -nc --arg a "$ADDR" --arg s "$sig" --slurpfile d "$out.data.json" '{address:$a,sig:$s,data:$d[0]}' > "$out.envelope.json"
  if [ "${DRYRUN:-0}" = 1 ]; then echo "DRYRUN: built+signed $out.envelope.json (types hash $th), not POSTed"; echo '{"id":"0xDRYRUN"}' > "$out.response.json"; return 0; fi
  curl -s -m 60 -X POST "$SEQ" -H 'Accept: application/json' -H 'Content-Type: application/json' \
       --data-binary @"$out.envelope.json" | tee "$out.response.json"; echo
}
