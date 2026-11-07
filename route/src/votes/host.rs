//! The Bloom-backed [`VoteHost`]: the only code of the vote flow that calls
//! the Petal SDK. The signing calls are the ones Step 1 proved live
//! (the removed `hello.rs`, docs/plan-hello-signature.md), with the derived
//! nonce only.
//!
//! SDK calls (pinned rev ba76944, `crates/petal-sdk/src`):
//! - store: `sdk::store_get` (lib.rs:579), `store_put` (:592), `store_put_new`
//!   (:597), `store_del` (:602), `store_del_if_value` (:607), `store_list`
//!   (:612). A `creds/` key or `secret = true` selects `secrets`
//!   (`namespace_for_key`, :677-683). Bloom refuses `put_new` on an existing key
//!   and `del_if_value` on a changed value with `Denied`, and reports a missing
//!   key to `del_if_value` as `NotFound` (`bloom-petals/src/private_store.rs:84-161`).
//! - clock and randomness: `sdk::try_now_ms` (:650), `sdk::random_bytes` (:654),
//!   `bloom:env/runtime`, no capability (SKILL.md:297; `vm.rs:2345-2358`).
//! - HTTP: `sdk::http_fetch` (:405) via `crate::hub::fetch` and the sequencer POST.
//! - signing: `petal::petal_use_claim` (claim.rs:581), `PetalClaim::sign_request`
//!   (claim.rs:634), `ClaimEffects::moves_nothing` (claim.rs:216),
//!   `ClaimNonce::Derived` (claim.rs:435), `CryptoSuite::Secp256k1Keccak256Recoverable`
//!   (claim.rs:73), `sdk::sign_payload` → `SignOutcome` (lib.rs:443, 226).
//!   `approval_hint` is never set (SKILL.md:546-547).
//! - account: `WalletAccount::address_evm` (lib.rs:1537), a `vfs_read`.

use crate::Network;
use crate::hub::{self, HubError};
use crate::proposals::{HubProposal, ProposalData, proposal_query};
use crate::snapshot::{self, Vote};
use crate::votes::flow::{SIGN_INTENT, SignResult, VoteHost};
use crate::votes::hubq::{HubVote, VotesData, VpData, votes_query, vp_query};
use crate::votes::sequencer::{MAX_RESPONSE_BYTES, PostResult, SignatureBytes};
use crate::votes::store::{DelIfValue, KvStore, PutNew};

/// The host of one route call for one account.
pub struct SdkHost<'a> {
    ctx: &'a petal::Ctx,
    account: &'a petal::WalletAccount,
}

impl<'a> SdkHost<'a> {
    pub fn new(ctx: &'a petal::Ctx, account: &'a petal::WalletAccount) -> Self {
        Self { ctx, account }
    }
}

fn message(error: &petal::SdkError) -> String {
    // A fixed status word or the host's own text; never stored values.
    hub::sanitize(&error.message())
}

fn response_message(response: &petal::DispatchResponse) -> String {
    match response {
        petal::DispatchResponse::Error { message, .. } => hub::sanitize(message),
        _ => "unexpected host response".into(),
    }
}

fn require_secret_key(key: &str) -> Result<(), String> {
    if key.starts_with("creds/") {
        Ok(())
    } else {
        Err("secret keys must start with creds/".into())
    }
}

impl KvStore for SdkHost<'_> {
    fn store_get(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
        match petal::sdk::store_get(key, max_bytes) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(petal::SdkError::Host(petal::HostStatus::NotFound)) => Ok(None),
            Err(error) => Err(message(&error)),
        }
    }

    fn store_put(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
        petal::sdk::store_put(key, value, false).map_err(|error| message(&error))
    }

    fn store_put_new(&mut self, key: &str, value: &[u8]) -> Result<PutNew, String> {
        match petal::sdk::store_put_new(key, value, false) {
            Ok(()) => Ok(PutNew::Created),
            // Denied is also a namespace refusal: confirm the key exists.
            Err(petal::SdkError::Host(petal::HostStatus::Denied)) => match self.store_get(key, usize::MAX)? {
                Some(_) => Ok(PutNew::Exists),
                None => Err("store denied the write".into()),
            },
            Err(error) => Err(message(&error)),
        }
    }

    fn store_del_if_value(&mut self, key: &str, expected: &[u8]) -> Result<DelIfValue, String> {
        match petal::sdk::store_del_if_value(key, expected) {
            Ok(()) => Ok(DelIfValue::Deleted),
            Err(petal::SdkError::Host(petal::HostStatus::Denied)) => Ok(DelIfValue::Changed),
            Err(petal::SdkError::Host(petal::HostStatus::NotFound)) => Ok(DelIfValue::Missing),
            Err(error) => Err(message(&error)),
        }
    }

    fn store_del(&mut self, key: &str) -> Result<(), String> {
        if key.starts_with("creds/") {
            return Err("state deletes must not use creds/ keys".into());
        }
        match petal::sdk::store_del(key) {
            Ok(()) | Err(petal::SdkError::Host(petal::HostStatus::NotFound)) => Ok(()),
            Err(error) => Err(message(&error)),
        }
    }

    fn store_list(&mut self, prefix: &str, max_bytes: usize) -> Result<Vec<String>, String> {
        petal::sdk::store_list(prefix, max_bytes).map_err(|error| message(&error))
    }

    fn secret_put(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
        require_secret_key(key)?;
        petal::sdk::store_put(key, value, true).map_err(|error| message(&error))
    }

    fn secret_get(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
        require_secret_key(key)?;
        self.store_get(key, max_bytes)
    }

    fn secret_del(&mut self, key: &str) -> Result<(), String> {
        require_secret_key(key)?;
        match petal::sdk::store_del(key) {
            Ok(()) | Err(petal::SdkError::Host(petal::HostStatus::NotFound)) => Ok(()),
            Err(error) => Err(message(&error)),
        }
    }
}

impl VoteHost for SdkHost<'_> {
    fn now_ms(&mut self) -> Option<u64> {
        petal::sdk::try_now_ms().ok()
    }

    fn random16(&mut self) -> Option<[u8; 16]> {
        petal::sdk::random_bytes(16).ok().and_then(|bytes| bytes.try_into().ok())
    }

    fn account_address(&mut self) -> Result<String, String> {
        let raw = self.account.address_evm().map_err(|response| response_message(&response))?;
        snapshot::checksum_address(&raw).map_err(|reason| format!("account address.evm is invalid: {reason}"))
    }

    fn hub_proposal(&mut self, network: Network, id: &str) -> Result<Option<HubProposal>, HubError> {
        let query = proposal_query(id).map_err(|error| HubError::Transport(error.message().into()))?;
        hub::fetch::<ProposalData>(network, &query).map(|data| data.proposal)
    }

    fn hub_vp(&mut self, network: Network, voter: &str, space: &str, proposal: &str) -> Result<f64, HubError> {
        let data: VpData = hub::fetch(network, &vp_query(voter, space, proposal))?;
        data.vp.map(|vp| vp.vp).ok_or(HubError::Decode)
    }

    fn hub_votes(&mut self, network: Network, proposal: &str, voter: &str) -> Result<Vec<HubVote>, HubError> {
        let data: VotesData = hub::fetch(network, &votes_query(proposal, voter))?;
        Ok(data.votes)
    }

    fn sign(&mut self, vote: &Vote) -> SignResult {
        let digest = vote.digest();
        let payloads = [petal::PayloadSignItem { preimage: vote.preimage().to_vec(), claimed_hash: digest.0 }];
        let claim = match petal::petal_use_claim(
            self.ctx,
            SIGN_INTENT,
            petal::CryptoSuite::Secp256k1Keccak256Recoverable,
            &payloads,
            &petal::ClaimEffects::moves_nothing(),
            petal::ClaimNonce::Derived,
        ) {
            Ok(claim) => claim,
            Err(response) => return SignResult::Failed(format!("signing claim: {}", response_message(&response))),
        };
        let mut request = match claim.sign_request(self.account.wallet(), petal::SignSelector::Exact) {
            Ok(request) => request,
            Err(response) => return SignResult::Failed(format!("signing request: {}", response_message(&response))),
        };
        // Bind the readable vote to the approval (Machine records its sha256
        // in the exact facts). Deterministic for the frozen vote, so retries
        // match. approval_hint, advisory and key_ref_jcs stay None.
        request.action = serde_json::to_vec(&vote.typed_data()).ok();
        match petal::sdk::sign_payload(&request) {
            Ok(petal::SignOutcome::Signature(bytes)) => SignResult::Signature(SignatureBytes::new(bytes)),
            Ok(petal::SignOutcome::ApprovalPending { action_id, expires_ms }) => {
                SignResult::Pending { action_id, expires_ms }
            }
            Err(petal::SdkError::Host(petal::HostStatus::Denied)) => SignResult::Denied,
            // No signature exists when signing fails, so the text holds none.
            Err(error) => SignResult::Failed(message(&error)),
        }
    }

    fn post_vote(&mut self, network: Network, body: &[u8]) -> PostResult {
        let request = petal::sdk::HttpRequest {
            method: "POST".into(),
            url: network.sequencer_submit_url().into(),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "application/json".into()),
            ],
            body: body.to_vec(),
        };
        match petal::sdk::http_fetch(&request, MAX_RESPONSE_BYTES) {
            Ok(response) => PostResult::Http { status: response.status, body: response.body },
            Err(petal::SdkError::Host(petal::HostStatus::BufferTooSmall { .. })) => {
                PostResult::Transport("response too large".into())
            }
            // Host errors never echo the request body.
            Err(error) => PostResult::Transport(message(&error)),
        }
    }
}
