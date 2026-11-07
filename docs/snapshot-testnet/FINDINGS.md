# Step 0 — Snapshot testnet de-risk (FINDINGS)

**Status (2026-11-06): DONE.** A vote signed by the throwaway key was accepted by the testnet sequencer and counted on
the testnet hub (§8). It was cast in our own space, `bloom-ballot-182c.eth`, because no suitable existing test space was found (§4).
Totals: 1 ENS name, 1 space, 1 proposal, 1 vote, all on testnet/Sepolia. The only Bloom VFS write is `/tools/eip712/hash/step0/in.json`.

Sources (cloned 2026-11-06 into `vendor/`, not included in this export):
- `snapshot.js` @ `6ebeb614ad234f24fd1cdf18247f86a46583451d` (2026-10-29)
- `snapshot-sequencer` @ `f57788923b219d21cfcae7d8609ded20c01aa433` (2026-05-08).
  The deployed testnet sequencer reports `version "0.1.3#14783ac"` (see Hosts below), so the
  deployed commit is `14783ac`, not necessarily our clone's HEAD. Line citations refer to our clone.

Throwaway signer: `0x042abe4EE65cBC71Bd11297DCea2d69e1650a596`. The key was kept in a local chmod-600 file outside the repo (`$SNAPSHOT_KEY_FILE`) and was never printed.

Helper: `gql.sh '<query>'` POSTs to the testnet hub. Raw responses are in `responses/`.

## 1. Hosts
| Role | URL | Evidence |
|---|---|---|
| Testnet hub (GraphQL) | `https://testnet.hub.snapshot.org/graphql` | `POST` → `200 application/json`, `{"data":{"spaces":[{"id":"dry-run-test.eth"}]}}`. Also `vendor/snapshot.js/src/constants.json:7` |
| (wrong host) | `https://testnet.snapshot.org/graphql` | `POST` → `404 text/html` ("Page not found"). This is the UI, not the hub. |
| Testnet sequencer | `https://testnet.seq.snapshot.org/` | `GET /` → `{"name":"sequencer","SNAPSHOT_ENV":"testnet","version":"0.1.3#14783ac","relayer":"0x8BBE4Ac64246d600BC2889ef5d83809D138F03DF"}`; `POST /` with `{}` → `400 {"error":"client_error","error_description":"wrong envelope format"}`. Route: `vendor/snapshot-sequencer/src/api.ts:19-20` (`router.post('/')`), mounted at `/` in `src/index.ts:44`. Also `constants.json:8` |

snapshot.js `Client` rewrites a hub URL to its sequencer URL (`src/sign/index.ts:57-66`), so the "hub" passed to the
client is not where votes go.

## 2. Vote typed data, as current snapshot.js builds it
- Domain: `{name:"snapshot", version:"0.1.4"}` with no chainId and no verifyingContract (`src/sign/index.ts:40-51`).
  chainId is added only for Trust Wallet in the browser (`index.ts:85-87`). The sequencer rejects any other
  name/version with `wrong domain` (`snapshot-sequencer/src/ingestor.ts:18-24,72-74`).
- Types `voteTypes` (`src/sign/types.ts:204-215`). Order matters:
  ```
  Vote(string from,string space,uint64 timestamp,string proposal,uint32 choice,string reason,string app,string metadata)
  ```
- `vote()` defaults (`index.ts:164-181`): `reason=""`, `app=""`, `metadata="{}"`. Single-choice and basic use
  `voteTypes` (uint32 choice). approval and ranked-choice use `voteArrayTypes` (`uint32[]`). quadratic, weighted and shutter
  use `voteStringTypes` (JSON string). `type` and `privacy` are deleted from the message before signing.
- `sign()` (`index.ts:71-91`): `message.from` = checksummed address. `timestamp` = `parseInt((Date.now()/1e3).toFixed())`.
  The envelope is `{address: checksumAddress, sig, data: {domain, types, message}}`. `types` does NOT include `EIP712Domain`.
  The sequencer deletes it anyway (`ingestor.ts:77`).
- Choice is 1-indexed for single-choice and basic (`snapshot.js/src/voting/singleChoice.ts:21-26`).

### Differences from our spec
| Field | Spec | Current snapshot.js |
|---|---|---|
| `from` | `address` | **`string`** (checksummed hex) |
| `proposal` | `bytes32` | **`string`** (`0x…` proposal id) |
| everything else | same | same (order, uint64 timestamp, uint32 choice, domain) |

**Both variants are accepted.** The sequencer identifies the message type by `sha256(JSON.stringify(types))`
looked up in `hashedTypes.json` (`ingestor.ts:79-81`). With compact JSON in snapshot.js key order:
- current `string`/`string` types → `fb8fa981…c8c458553364ed809e45042f141de8c0d5` = `"vote"` (`src/sign/hashedTypes.json:73`)
- spec `address`/`bytes32` types → `bc8c6814…5b9b6ef6415c656c9e7b9e05e029cc19b954b7160dc2928a12caf` = `"vote"` (`hashedTypes.json:46`, legacy)

The two produce **different EIP-712 digests**. Recommendation: use the current snapshot.js variant (`string`/`string`).
Risk: the hash is taken over our *JSON serialization*. The Rust side must emit `types` byte-identical to
`{"Vote":[{"name":"from","type":"string"},…]}`, with keys `name` then `type`, no whitespace, and no `EIP712Domain`.
Otherwise the result is `wrong types`.
(Whether the deployed sequencer's bundled snapshot.js contains both hashes: unverified until a submission.)

## 3. Sequencer validation rules (vote path)
- **Timestamp tolerance** (`snapshot-sequencer/src/ingestor.ts:57-67`): `over = 300` (≤ 5 min in the future),
  `under = 60*60*24*6` (≤ 6 days in the past). Outside that window → `wrong timestamp`.
- Envelope schema (`src/helpers/envelope.json`): `address`, `sig`, `data`. `data` takes only `domain`, `message`, `types` and
  optionally `primaryType` (`additionalProperties:false`). `message.from` and `message.timestamp` are required.
- Body length must be ≤ 1e5 chars (`ingestor.ts:64`). The proposal id must not contain whitespace (`:69-70`). metadata must be ≤ 2000 chars (`:173-174`).
- Vote writer (`src/writer/vote.ts`):
  - schema: `app` maxLength 24, `reason` maxLength 5000 (`snapshot.js/src/schemas/vote.json:27-36`). `"bloom-ballot"` (12) fits.
  - The voting window applies to both message ts and server time: `start ≤ ts ≤ end` (`vote.ts:30-38`) → `not in voting window`.
  - choice validity (`:45-46`) → `invalid choice`.
  - The proposal's vote validation (copied from the space's `voteValidation` at creation, `writer/proposal.ts:254`) is run unless it is `any` (`vote.ts:49-76`) → `failed vote validation`.
  - vp via score API; `vp === 0` → `no voting power` (`vote.ts:78-98`).
  - The vote id is `snapshot.utils.getHash(body.data, body.address)` (`ingestor.ts:119`). A duplicate gives `duplicate message`.
  - The success response is `{id, ipfs, relayer:{address, receipt}}` (`ingestor.ts:284-291`).
- Proposal creation rules (relevant if we create one), in `src/writer/proposal.ts`: `onlyMembers` (`:152-160`),
  proposal validation (`:161-197`), day/month/active-per-author limits (`:219-229`), and a **premium-network check
  for non-turbo spaces** (`:41-57,88`), which may reject Sepolia-network spaces (unverified).

## 4. Space and proposal search (testnet hub)
- Active proposals on the whole testnet hub: **1** (`responses/active_proposals.json`): `newgoldprotocol.eth`,
  `erc20-balance-of` on chain 97. A new address would have vp 0 there. **So any vote requires creating a proposal.**
- Total spaces: 957 (`responses/all_spaces.ndjson`). Spaces with a `ticket` strategy: 33 (`responses/ticket_spaces.json`).
- Spaces that are open to proposals (`onlyMembers:false`) with `ticket`, and why they pass or fail for a new address:
  | space | proposal validation | vote validation | verdict |
  |---|---|---|---|
  | cryptolootlegends.eth (sepolia) | basic minScore 1 (ticket value 1) | basic minScore 1 | **passes both**, but it is a real community, see below |
  | worldocean.eth | basic 0.1 (ticket) | basic **minScore 5** | vote fails |
  | sunshine.eth | basic 500 | basic 500 | fails |
  | coolspace.shib | basic over contract-call | basic 0.1 | propose fails |
  | tortoise.eth | basic over erc1155 | basic over erc1155 | fails |
  | coinbase.globaldemocracy.eth | karma-eas-attestation | basic 1 | propose fails |
  | cross24.eth / valerybulav.eth | passport-gated | passport-gated | fails |
- Test-named spaces (`test|demo|sandbox|dry|playground`): all of them use token, whitelist or contract strategies (new address vp 0)
  and/or `onlyMembers:true`. `myvote-demo.eth` is open with `any` validation but uses a `whitelist` strategy.
  The `ticket-validity` spaces (`tekycrypto.eth`, `multinimemetateky.eth`) need DAI holdings.
- **Why I stopped:** `cryptolootlegends.eth` is the only space where the throwaway can both propose and vote, but it
  is **not clearly a test space**. It has 5 past proposals, all by `0x357Dad45…726C`, that are real community decisions
  ("CCL Community Round #2 Core Members Reward Plan", "Allocation of Treasury Revenue"), and its categories are `social, service`.

## 5. GraphQL queries (all verified against the testnet hub)
Voting power (`responses/vp_query.graphql` → `responses/vp_result.json`):
```graphql
{ vp(voter: "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596", space: "cryptolootlegends.eth", proposal: "0xd03f82218edb4ead3cc086bf3ac8936cb275b62301039a6fd30f83631ac453fe") { vp vp_by_strategy vp_state } }
```
→ `{"data":{"vp":{"vp":1,"vp_by_strategy":[1],"vp_state":"final"}}}`. This confirms that a brand-new address gets vp 1 via `ticket`.

Proposals of a space, active (`responses/q_proposals_active.*`) → `{"proposals":[]}`:
```graphql
{ proposals(first: 20, where: {space: "<space>", state: "active"}, orderBy: "created", orderDirection: desc) { id title type start end state } }
```
Proposals of a space, recent (`responses/q_proposals_recent.*`): the same query without `state`.

One proposal (`responses/q_proposal.*`):
```graphql
{ proposal(id: "<id>") { id space { id } author title body choices type start end snapshot state scores scores_total } }
```
→ e.g. `type:"basic", choices:["同意 For","反对 Against"], snapshot:7553557, state:"closed", scores:[4,0], scores_total:4`.

Votes by voter on a proposal (`responses/q_votes_by_voter.*`):
```graphql
{ votes(first: 10, where: {proposal: "<id>", voter: "<address>"}) { id voter choice vp created reason app } }
```
→ `{"id":"0xf0452f…d91e","voter":"0x357Dad…726C","choice":1,"vp":1,"created":1737630493,"reason":"","app":"snapshot-v2"}`.

## 6. Golden vector (offline, never submitted). Files in `golden/`
Built by `golden/build_vector.sh`. The values are synthetic: space `bloom-ballot-test.eth`, proposal = `keccak("bloom-ballot golden vector")`,
and a fixed timestamp of `1793923200` (2026-11-06T00:00:00Z).

Typed data (`golden/typed_data.json`. The envelope `data` in `golden/envelope_data.json` is the same minus `EIP712Domain` and `primaryType`):
```json
{
  "domain": {
    "name": "snapshot",
    "version": "0.1.4"
  },
  "types": {
    "EIP712Domain": [
      {
        "name": "name",
        "type": "string"
      },
      {
        "name": "version",
        "type": "string"
      }
    ],
    "Vote": [
      {
        "name": "from",
        "type": "string"
      },
      {
        "name": "space",
        "type": "string"
      },
      {
        "name": "timestamp",
        "type": "uint64"
      },
      {
        "name": "proposal",
        "type": "string"
      },
      {
        "name": "choice",
        "type": "uint32"
      },
      {
        "name": "reason",
        "type": "string"
      },
      {
        "name": "app",
        "type": "string"
      },
      {
        "name": "metadata",
        "type": "string"
      }
    ]
  },
  "message": {
    "from": "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596",
    "space": "bloom-ballot-test.eth",
    "timestamp": 1793923200,
    "proposal": "0x3b1261975f64a6e11b3f365280580fbe0e7e7589799f122f4c0bcd61021bdcc6",
    "choice": 1,
    "reason": "",
    "app": "bloom-ballot",
    "metadata": "{}"
  },
  "primaryType": "Vote"
}
```
| item | value |
|---|---|
| `types` sha256 (sequencer lookup) | `fb8fa9816cd42974e7f1af671aa548c8c458553364ed809e45042f141de8c0d5` → `"vote"` (`hashedTypes.json:73`) |
| Vote typeHash | `0x7bf372d792402c0796ec3567238391d04616792abb3fe96c99adc02d8b8cdffa` |
| domainSeparator | `0x484fce18f892e8535a4b6700e197a8026f4213f809d23ae117da03b497e18670` |
| structHash | `0x2b0bbfd2824580ad8696e79f475d4cd35594bcafb3fafac700bc50bd97f65f08` |
| **digest (Bloom `/tools/eip712/hash`)** | `0x9431fb703c91b4a1c216343688a73580e102f80585c4511376f7b58281ff0251` |
| **digest (independent, `golden/digest_manual.sh`)** | `0x9431fb703c91b4a1c216343688a73580e102f80585c4511376f7b58281ff0251` ✅ equal |
| signature (`cast wallet sign --data --from-file`) | `0x926a229e712f33c2fbdd2e60cb676e2332d0e7768b2ff88070a0c69c37d2e51542693fedf2e7eec85e8f7e57d32228da34ffdae7d6c3ac1f8f5e8db638b46cab1c` |
| signer | `0x042abe4EE65cBC71Bd11297DCea2d69e1650a596` |

Verification:
- `cast wallet verify --address <signer> --data --from-file typed_data.json <sig>` → "Validation succeeded".
- `cast wallet verify --address <signer> --no-hash <independent digest> <sig>` → "Validation succeeded". This recovers from the raw digest.
- Negative control: the same check against `0x…0001` → "Validation failed".

How each digest was computed:
- Bloom: `bloom vfs write /tools/eip712/hash/step0/in.json < golden/typed_data.json`, then `bloom vfs cat …/out.hex`.
  The implementation is alloy `TypedData::eip712_signing_hash` (bloom repo `crates/bloom-tools/src/abi.rs:83-90`).
- Independent: a hand-rolled hashStruct using only `cast keccak` and `cast abi-encode`. There is no typed-data library involved.

**Pitfall found while doing this.** `cast keccak "0x…"` hashes the *hex bytes*, not the UTF-8 string. The first manual
attempt therefore produced a different digest. EIP-712 `string` fields (`from`, `proposal`) must be hashed as
`keccak(utf8(s))`, even though they look like hex. The Rust code must do the same: no hex-decoding of `from` or `proposal`.
`from` must be the **checksummed** string (snapshot.js `sign()` uses `getFormattedAddress`). A lowercase `from`
produces a different digest.

## 7. Our own testnet space: plan (executed 2026-11-06, see §8). See `space/README.md`
Throwaway address to fund: **`0x042abe4EE65cBC71Bd11297DCea2d69e1650a596`**. Space id: **`bloom-ballot-182c.eth`**.

**How Snapshot decides who controls a space** (no text record needed):
- The sequencer's `settings` writer accepts a signed `Space` message from the space controller or an existing admin
  (`snapshot-sequencer/src/writer/settings.ts:42-65`).
- The controller comes from `getSpaceController(space, DEFAULT_NETWORK)` (`snapshot-sequencer/src/helpers/utils.ts:287-300`)
  → snapshot.js `getEnsSpaceController` (`snapshot.js/src/utils.ts:576-591`). It uses the ENS text record `snapshot`
  if one is set, otherwise the ENS owner (`src/utils/ens.ts:225-338`). On Sepolia the owner is read through ENSv2
  `UniversalHelper.findExactOwner`, with a v1 fallback. Sepolia config is in `src/networks.json` (`11155111.ensUniversalHelper` = `0xd453e5Bd…7DFe`).
  A missing resolver counts as "no record" (`ens.ts:69-84`), so it falls back to the owner.
- The testnet sequencer resolves on **Sepolia ENS**. `DEFAULT_NETWORK` is an env var (unverified directly), but the evidence is strong:
  `dry-run-test.eth` (created on the testnet hub 2026-10-21) has a Sepolia ENSv2 owner `0x44fd1d3b…2a7f` equal to its
  Snapshot admin, and its mainnet v1 owner is `0x0`.
- Space EIP-712 types: `Space(address from,string space,uint64 timestamp,string settings)`. Note **`address`** here, unlike Vote
  (`snapshot.js/src/sign/types.ts:131-138`). Hash `110b37d3…5666` → `"settings"` (`hashedTypes.json:2`).
- Proposal types: `snapshot.js/src/sign/types.ts:140-158`. Hash `e037b373…4097` → `"proposal"` (`hashedTypes.json:69`).
- Proposal creation needs the space networks to be "premium" (`writer/proposal.ts:41-57,88`). The list is DB-only
  (`helpers/actions.ts:150-152`), but non-turbo Sepolia spaces created proposals as recently as 2026-10-29
  (`responses/recent_proposals.json`), so Sepolia is premium on testnet.

**ENS on Sepolia is ENSv2.** The docs say: "These ENSv1 contracts still exist on Sepolia but are no longer in use" and "new
registrations happen exclusively in ENSv2" (docs.ens.domains/learn/deployments, /web/ensv2-readiness).
I verified these on-chain on 2026-11-06 via `https://ethereum-sepolia-rpc.publicnode.com`:
- ETHRegistrar `0xf633e7fc17e2bbe0d0965d18ec1821dcb754a3d3`: `MIN_COMMITMENT_AGE` 60, `MAX_COMMITMENT_AGE` 86400,
  `MIN_REGISTER_DURATION` **2419200 (28 days)**, rentPriceOracle `0x8196…263A`. `isAvailable("bloom-ballot-182c")` = true.
- Flow: `makeCommitment(string label,address owner,bytes32 secret,address subregistry,address resolver,uint64 duration,bytes32 referrer)`
  → `commit(bytes32)` → wait ≥ 60 s → `register(label,owner,secret,subregistry,resolver,duration,address paymentToken,bytes32 referrer)`.
  Payment is ERC-20 via `safeTransferFrom` (approve first). ETH is not accepted.
- Accepted payment tokens, checked via `getRegisterPrice`: MockUSDC `0x240b0316…14aa` (deployments page) and Circle Sepolia USDC
  `0x1c7D4B19…7238`, each about 8.000021 USDC/yr. The readiness page's test USDC `0x16f95d…aa8e` and test DAI `0x278053…9664`
  **revert** (`0x02e2ae9e`, token not supported), so the two doc pages disagree and the deployments page is correct.
  MockUSDC `mint(address,uint256)` is open, as an `eth_call` from the throwaway succeeds.
- 90-day price: 1,972,608 MockUSDC units (≈ 1.97 USDC). It's free to mint.

**Sepolia ETH needed.** `cast estimate`: mint 141,730, approve 129,366, commit 128,547. Register is budgeted at ≤ 1,000,000 because it can't be estimated before
a commit exists. That's about 1.4M gas in total. The gas price was ~20 wei at the time. **Recommend funding 0.05 SepoliaETH** (safe up to ~35 gwei).
Steps 5–7 (space, proposal, vote) are gasless.

**Offline verification of steps 5–7:** `space/dryrun_test.sh` builds, signs and verifies all three envelopes. Their types hashes
match the sequencer's `settings`, `proposal` and `vote` entries.

Confirmed at execution: `register` accepts `subregistry=0`/`resolver=0`, and Snapshot falls back to the ENS owner as controller.

## 8. Live run (2026-11-06). Values below are copied from the local run logs (not exported)
Hosts used: sequencer `POST https://testnet.seq.snapshot.org/`; hub `POST https://testnet.hub.snapshot.org/graphql`.

**ENS (Sepolia, ENSv2)**: name **`bloom-ballot-182c.eth`**, registered for 90 days and paid in MockUSDC.
| step | tx | result |
|---|---|---|
| MockUSDC mint | `0xa978eee7e3677e6f0742910f64faf3cabdf4d84e511a43326e560c40f572af54` | status 1 |
| approve ETHRegistrar | `0xffdc98e5405bee2989167d7e668c8a8c91242e989e6c21965e798992e47138c9` | status 1 (the first `allowance` read showed 0 because of RPC lag; a re-read gave 3,945,216) |
| commit | `0x4f11f0967bb0a521fcdc4d94ad48bbfa98554ba0acb331b3ff0ed01f445d850e` | commitment `0xc5123efb…d6af` |
| register | `0x5bb66945ff8019bbcd39fd3d25eab58028a2f41f1e8d03b5b03d5ed489261233` | status 1, gasUsed 679,217. `UniversalHelper.findExactOwner` = throwaway |
Total ETH spent: 0.05 → 0.049946224282791776 (≈ 0.0000538 ETH).

**Sequencer responses** (`{id, ipfs, relayer:{address, receipt}}`, relayer `0x8BBE4Ac64246d600BC2889ef5d83809D138F03DF`):
| message | id | ipfs | receipt |
|---|---|---|---|
| space settings | `0xb843b0ad38f86fda859278c4ca7429dc8782d74340295604fc3f333b834794f5` | `bafkreibj2kppemkn4vxjzt5w6vxm66enghrys2jjx3urw67ve52c5lqeiu` | `0x74ef2201cbf2a66eda139f87dc12dc08693ca6c67645318e3da4583149153146233d13a7b2f42d10d9cf6dce0bc71a4aeac133205e49972dc023b8a65a3d57091b` |
| proposal | `0x8ef55d4139e0c94f83b25aea7d4a7d573092eef7da4bc8ec8a54faab9d644b00` | `bafkreicnipwjgutthdo2qezqnpoibm7vvtao5m7s7qkavftqx7ehptyw2a` | `0x2aec476a0ea3029110af6ffa50d3422404dcbd6c4786773c0734eed82aaa25335c51f9dc78252133556763de70e19e94080c2bb49adbd3d5325bf1c6e3840b091c` |
| **vote** | `0xd74075ee566b3ee70e6fa9db10c4b491fe814549e1f5e3507a32f03b63c9a657` | `bafkreigs5zwiono6neqpkz5ye5py757jzge3j4ftguigcuwi6zbx6dhi74` | `0x2eb06d05c25da5a1632997a7258d9f9ed2b8807e23ef6f189a8c9ede0443ea1c34c333362f5184cc60c9b6aeccdcf34cd8e3ba972a370adc373355f9ef3e3d9b1b` |
The returned `id` is the hub's object id (proposal id / vote id), i.e. `getHash(data, address)` (`ingestor.ts:119`).

**Space on hub**: `bloom-ballot-182c.eth`, admins `[throwaway]`, `ticket` (value 1), validation `any`, voteValidation `any`,
`onlyMembers: true`, voting type `single-choice`.
**Proposal on hub**: `0x8ef55d41…4b00`, "bloom-ballot test", single-choice `["For","Against","Abstain"]`,
start 1793988890, end 1794593689 (7 days), snapshot block 12078262, state `active`.
**vp before voting**: `{"vp":1,"vp_by_strategy":[1],"vp_state":"final"}`.

**Hub confirmation (Verify)**:
```bash
./gql.sh '{ votes(where: {proposal: "0x8ef55d4139e0c94f83b25aea7d4a7d573092eef7da4bc8ec8a54faab9d644b00", voter: "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596"}) { id voter choice vp created app } proposal(id: "0x8ef55d4139e0c94f83b25aea7d4a7d573092eef7da4bc8ec8a54faab9d644b00") { state scores scores_total votes } }'
```
→ (`responses/final_hub_check.json`)
`votes: [{id:"0xd74075ee…a657", voter:"0x042abe4EE65cBC71Bd11297DCea2d69e1650a596", choice:1, vp:1, created:1793988894 (2026-11-06T18:14:54Z), app:"bloom-ballot"}]`,
`proposal: {state:"active", scores:[1,0,0], scores_total:1, votes:1}`. **The vote is counted.**
The vote was visible on the first hub poll, right after the sequencer response.

The deployed sequencer accepted the exact snapshot.js `voteTypes` (hash `fb8fa981…`), which confirms §2's recommendation
against the live service.

## 9. Extra test proposals (2026-11-06, `space/08_new_proposal.sh`, NOT voted on)
All three were created by the throwaway admin in `bloom-ballot-182c.eth`: single-choice, For/Against/Abstain, `app:"bloom-ballot"`, snapshot block 12078297.
| purpose | title | id | start | end | receipt |
|---|---|---|---|---|---|
| mandate-breach test | Treasury transfer to grants multisig | `0x8c45d4c1f422023c56ba41b41d00f441df20b0a14ae22a0eab5e1eb77da505e1` | 1793989311 | 1794594110 (7 d) | `0x92762eca…b469ee1c` |
| main demo | Lower protocol fee to 0.2% | `0x5fa4f371c47a9691128eaa0bc079abed859f73b9f2e0afb453ff5a7d5b752607` | 1793989313 | 1794594112 (7 d) | `0x3061865a…ee2ded211c` |
| closed-proposal test | Short-lived test | `0xb2a3f2fba4445f4672c2b2151ba32675ca5e3144f22910205ea1b159300425f2` | 1793989315 | 1793989914 (10 min, closes 2026-11-06T18:31:54Z) | `0xac6a54f9…fbbe1b7b01c` |
The full sequencer responses are in local run state (not exported).
Idempotency was checked: re-running with the existing title "bloom-ballot test" was refused (exit 3) before signing.
The hub's `start` was 1 s later than our signed `start` (1793989311 vs 1793989310). `end` matched exactly. Read the times back from the hub instead of trusting local values (cause unverified).

## Surprises / risks
- The testnet hub has almost no live activity (1 active proposal across 957 spaces). Our integration test cannot
  rely on finding one, so we need a space we control or an agreed sandbox.
- The type-hash matching means the exact JSON of `types` is part of the protocol.
- The spec's `address`/`bytes32` typing is a legacy variant, still accepted but with a different digest.
- The premium-network check on proposal creation: Sepolia appears to be premium on testnet (see §7).
- ENS docs conflict on the Sepolia test-token addresses. Only the deployments-page MockUSDC and Circle USDC are accepted on-chain.
- The live sequencer accepted the `settings`, `proposal` and `vote` type hashes we computed from our clone, so its bundled snapshot.js matches on these types.
- The vote counts against the 3-vote budget: 1 of 3 is used. The proposal stays active until 1794593689 (2026-11-13). A re-vote replaces the earlier vote only if its timestamp is later. An older timestamp is rejected with `already voted at later time`, and an equal one is decided by comparing ids (`writer/vote.ts:139-152`). This matters for idempotency and retries in the petal.

## 10. Vote-flow hub queries with GraphQL variables (recorded 2026-11-07)
Recorded with `gqlv.sh <query.graphql> '<variables>'`, the request shape `route/src/hub.rs` sends
(`{"query": <constant>, "variables": {...}}`). Nothing was signed or submitted.

| query (`responses/`) | variables | response file | result |
|---|---|---|---|
| `ballot_vp.graphql`: `query Vp($voter: String!, $space: String!, $proposal: String) { vp(voter: $voter, space: $space, proposal: $proposal) { vp vp_by_strategy vp_state } }` | Bloom account `0x0c056ae6…2751`, `bloom-ballot-182c.eth`, "bloom-ballot test" `0x8ef55d41…4b00` | `ballot_vp_testnet.json` | `vp 1, vp_state "final"` (ticket strategy) |
| same | same account, unknown proposal `0x…00ff` | `ballot_vp_unknown_proposal.json` | GraphQL error `proposal not found`, `data: null` |
| `ballot_votes.graphql`: `query Votes($proposal: String!, $voter: String!) { votes(first: 10, where: {proposal: $proposal, voter: $voter}) { id voter choice created } }` | "bloom-ballot test", throwaway (checksummed) | `ballot_votes_testnet.json` | one vote, id `0xd74075ee…a657`, choice 1 |
| same | "bloom-ballot test", Bloom account | `ballot_votes_testnet_empty.json` | `[]` (has not voted) |
| same | "bloom-ballot test", throwaway all lowercase | `ballot_votes_testnet_lowercase_voter.json` | the same vote |

**Voter filter case sensitivity:** the hub matches `voter` regardless of the case of the 40 hex digits:
checksummed, all-lowercase and all-uppercase hex (`0x` + upper) return the same vote, and the hub
returns `voter` checksummed. Only the prefix is significant: `0X…` returns `[]`. Ballot always sends the
EIP-55 checksummed address with a lowercase `0x`, the same string it signs as `from` and sends as the
envelope `address` (the hub stores that address, `snapshot-sequencer/src/writer/vote.ts:107`).

**Vote id = EIP-712 digest:** `golden/digest_manual.sh` over the §8 vote message (`golden/live_vote_message.json`)
prints `0xd74075ee566b3ee70e6fa9db10c4b491fe814549e1f5e3507a32f03b63c9a657`, the id above, as expected from
`ingestor.ts:119` → snapshot.js `src/verify/evm.ts:17-20` (`_TypedDataEncoder.hash`).

## 11. Inbox hub queries (recorded 2026-11-08)
Same request shape as §10 (`gqlv.sh`). Nothing signed or submitted.

| query (`responses/`) | variables | response file | result |
|---|---|---|---|
| `ballot_inbox.graphql`: `open` = active proposals of a space by `end` asc (first 20), `recent` = its 10 newest, each with `scores` | `bloom-ballot-182c.eth` | `ballot_inbox_testnet.json` | 3 open (test proposal scores `[2,0,0]`), 4 recent incl. the closed "Short-lived test" (no votes, `scores: []`) |
| same | `no-such-space-xyz.eth` | `ballot_inbox_unknown_space.json` | both lists empty, no error |
| `ballot_voted.graphql`: `votes(first: 50, where: {voter: $voter, proposal_in: $proposals})` with `$proposals: [String]!` | Bloom account `0x0c056ae6…2751` (checksummed), the four test-space proposals | `ballot_voted_testnet.json` | one vote: the accepted live vote on "bloom-ballot test", choice 1 |
| same | throwaway, all lowercase | `ballot_voted_testnet_lowercase_voter.json` | its Step 0 vote (`0xd74075ee…`): `proposal_in` works and `voter` is case-insensitive here too |
