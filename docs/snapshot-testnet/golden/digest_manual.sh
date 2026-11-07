#!/usr/bin/env bash
# Independent EIP-712 digest: hand-rolled hashStruct with cast keccak + abi-encode (no typed-data library).
set -euo pipefail
M=${1:-message.json}
k() { cast keccak "$(cast from-utf8 "$1")"; }   # keccak256 of UTF-8 bytes (cast keccak would treat "0x.." input as hex bytes)
DOMAIN_TH=$(k "EIP712Domain(string name,string version)")
DS=$(cast keccak "$(cast abi-encode 'f(bytes32,bytes32,bytes32)' "$DOMAIN_TH" "$(k snapshot)" "$(k 0.1.4)")")
VOTE_TH=$(k "Vote(string from,string space,uint64 timestamp,string proposal,uint32 choice,string reason,string app,string metadata)")
g() { jq -r ".$1" "$M"; }
SH=$(cast keccak "$(cast abi-encode 'f(bytes32,bytes32,bytes32,uint64,bytes32,uint32,bytes32,bytes32,bytes32)' \
  "$VOTE_TH" "$(k "$(g from)")" "$(k "$(g space)")" "$(g timestamp)" "$(k "$(g proposal)")" "$(g choice)" \
  "$(k "$(g reason)")" "$(k "$(g app)")" "$(k "$(g metadata)")")")
echo "typeHash        $VOTE_TH" >&2
echo "domainSeparator $DS" >&2
echo "structHash      $SH" >&2
cast keccak "0x1901${DS#0x}${SH#0x}"
