# Evidence

Live JSON outputs copied unchanged from runs on the developer triad against
Snapshot testnet. Wallet `main`, account `0`.

Checked before copying: valid JSON, no ceremony URLs, no signature bytes, no
private paths. Reads carry at most `signature_sha256`. `scripts/leak-check.sh`
passes, with three entries added to `scripts/leak-allowlist.txt`: the 132-digit
`preimage` values in `accept-1.json`, `rehearsal-follow-vote.json` and
`step1-hello-sign.json`. Those are public (66 bytes, `0x1901` prefix), and
`keccak256` of each equals the file's digest.

## Agent rehearsal, 2026-11-09

A screenshot of the Snapshot site, not a JSON output.

| File | What it proves |
| --- | --- |
| `snapshot-vote.png` | The Bloom account `0x0c056ae661D3d3f87919eCb9733527509D392751` voted For on "Agent rehearsal: adopt monthly community calls" in `bloom-ballot-182c.eth` on testnet.snapshot.box. The governance agent cast it during the agent rehearsal. Digest `0x73f0f36db95446efd64f56a879d5ae8356f272795c901fc2dfff292b8c5845c1`. |

The image shows the space's display name ("bloom-ballot test"), the voter as
`0x0c05…2751`, the choice, the reason and the timeline. The proposal title, the
space id and the digest are not in the frame.

## Dress rehearsal, 2026-11-09

| File | What it proves |
| --- | --- |
| `rehearsal-follow-vote.json` | A followed delegate's vote went from drafted to `counted` through the owner's ceremonies. `digest`, `receipt.id`, `counted.hub_vote_id` and `live.hub_vote.id` are all `0x88ad…9f9a`. |
| `rehearsal-inbox.json` | After that vote, the inbox shows the rehearsal `voted:true` with `our_choice` For, and a `delegate_voted` alert for "Lower protocol fee to 0.2%". |
| `rehearsal-following.json` | The voted proposal is no longer offered. Only Lower-fee remains, with the suggested body `follow-5fa4f371`. |
| `rehearsal-treasury-refused.json` | `mandate_blocked_keyword` on "Treasury transfer to grants multisig": `bound:false`, no approval, no digest, no receipt. |

## Live acceptance, 2026-11-07

| File | What it proves |
| --- | --- |
| `accept-1.json` | The first full vote reached `counted`, with one submit attempt. Digest, receipt and hub id are all `0x891b…2a2e`. |
| `closed-1.json` | A refused `vote_id` is not bound: reusing `closed-1` with choice 9 gave `choice_out_of_range`. |
| `votes-new-marker.json` | Read-after-write marker for a conflicting body: `outcome:conflict`, `code:-3`, `accept-1 is bound to a different vote`. |
| `mandate.json` | A saved mandate with its rules, enforcement notes and `last_write`. |
| `inbox.json` | The inbox for that mandate, with `voted` and `our_choice` from the hub. |
| `treasury-1-refused.json` | The mandate's blocked keyword refused a vote with no binding and no approval. |

## Signing pipeline proof, 2026-11-06

These come from an earlier build. The `hello-sign.json` route no longer exists.

| File | What it proves |
| --- | --- |
| `step1-hello-sign.json` | Bloom produced a signature for `snapshot.vote` that recovered to the account. It was never submitted. |
| `step1-policy.json` | After the owner's policy ceremony, the wallet policy lists that build's package hash in `allowed_petal_packages`. |

## What these files do not contain

- **The output of Bloom's `/tools/eip712/hash` and the hash shown on the ceremony page.** Neither was saved. Reproduce the first with the commands in the README's "Verify without trusting Ballot".
- **The passkey ceremonies themselves.** They appear only indirectly, through `history` and the resulting signature fingerprint.

Separate checks on 2026-11-09 for the rehearsal vote: the hub returned the same
id, voter, choice, reason and ipfs hash, and an offline recomputation from
`.typed_data` with Foundry's `cast` (keccak and ABI encoding, no Ballot or Bloom
code) gave `0x88ad…9f9a`.
