//! Store layout, the store trait the vote flow runs against, and the
//! per-vote lease (docs/plan-votes.md §1).
//!
//! Layout, with `A = <network>/<wallet>/<index>`:
//!
//! | key | namespace | written with |
//! | --- | --- | --- |
//! | `votes/A/<vote_id>/binding.json` | state | `put_new` once, never changed |
//! | `votes/A/<vote_id>/record.json` | state | `put` |
//! | `votes/A/<vote_id>/lease` | state | `put_new` / `del_if_value` |
//! | `markers/A/new.json` | state | `put` |
//! | `creds/votes/A/<vote_id>/signature` | secrets | `put(.., secret = true)`, deleted once final |
//!
//! The SDK sends a key to the `secrets` namespace when `secret` is true or the
//! key starts with `creds/`, and its get/delete/list always pass
//! `secret = false` (SDK ba76944 `lib.rs:579-617`, `namespace_for_key`
//! `:677-683`), so a secret is reachable only under a `creds/` key. Bloom
//! gives every account its own store (`bloom-petals/src/private_store.rs:192-199`);
//! keys still carry the account as defence in depth.

use serde::{Deserialize, Serialize};
use serde::de::DeserializeOwned;

use crate::Network;

/// Largest stored JSON document read back (binding, record, marker, lease).
pub const MAX_DOC_BYTES: usize = 64 * 1024;
/// Largest total size of key names returned by one listing.
pub const MAX_LIST_BYTES: usize = 64 * 1024;
/// A lease older than this is abandoned (a crashed or killed write) and may
/// be taken over.
pub const LEASE_STALE_MS: u64 = 120_000;

/// The account a vote belongs to: the network segment plus Bloom's
/// host-resolved wallet id and account number (`petal::wallet_account`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Scope {
    pub network: Network,
    pub wallet: String,
    pub index: u32,
}

impl Scope {
    pub fn new(network: Network, wallet: &str, index: u32) -> Self {
        Self { network, wallet: wallet.to_owned(), index }
    }

    fn base(&self) -> String {
        format!("{}/{}/{}", self.network.as_str(), self.wallet, self.index)
    }

    /// Prefix of every vote key of this account (for listing).
    pub fn votes_prefix(&self) -> String {
        format!("votes/{}/", self.base())
    }

    pub fn binding_key(&self, vote_id: &str) -> String {
        format!("votes/{}/{vote_id}/binding.json", self.base())
    }

    pub fn record_key(&self, vote_id: &str) -> String {
        format!("votes/{}/{vote_id}/record.json", self.base())
    }

    pub fn lease_key(&self, vote_id: &str) -> String {
        format!("votes/{}/{vote_id}/lease", self.base())
    }

    pub fn marker_key(&self) -> String {
        format!("markers/{}/new.json", self.base())
    }

    /// Secret-namespace key of a vote's signature (the `creds/` prefix is what
    /// routes it to `secrets` for get and delete).
    pub fn signature_key(&self, vote_id: &str) -> String {
        format!("creds/votes/{}/{vote_id}/signature", self.base())
    }

    /// VFS directory of this account's votes.
    pub fn votes_dir(&self) -> String {
        format!(
            "/petals/ballot/{}/accounts/{}/{}/votes",
            self.network.as_str(),
            self.wallet,
            self.index
        )
    }

    /// Vote ids named by a listing of [`Scope::votes_prefix`]: every
    /// `<prefix><id>/binding.json` or `<prefix><id>/record.json`, sorted, once.
    pub fn vote_ids_from_keys(&self, keys: &[String]) -> Vec<String> {
        let prefix = self.votes_prefix();
        let mut ids: Vec<String> = keys
            .iter()
            .filter_map(|key| key.strip_prefix(&prefix))
            .filter_map(|rest| {
                rest.strip_suffix("/record.json").or_else(|| rest.strip_suffix("/binding.json"))
            })
            .filter(|id| crate::validate_vote_id(id).is_ok())
            .map(str::to_owned)
            .collect();
        ids.sort();
        ids.dedup();
        ids
    }
}

/// Result of a create-if-absent write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PutNew {
    Created,
    /// The key already exists; nothing was written.
    Exists,
}

/// Result of a compare-and-delete.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DelIfValue {
    Deleted,
    /// The stored value differs; nothing was deleted.
    Changed,
    Missing,
}

/// The private key-value store, as the vote flow uses it. Errors are short,
/// sanitized messages (never stored values). State methods use the `state`
/// namespace; `secret_*` methods use `secrets` and take only
/// [`Scope::signature_key`] keys.
pub trait KvStore {
    fn store_get(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, String>;
    fn store_put(&mut self, key: &str, value: &[u8]) -> Result<(), String>;
    fn store_put_new(&mut self, key: &str, value: &[u8]) -> Result<PutNew, String>;
    fn store_del_if_value(&mut self, key: &str, expected: &[u8]) -> Result<DelIfValue, String>;
    fn store_list(&mut self, prefix: &str, max_bytes: usize) -> Result<Vec<String>, String>;
    fn secret_put(&mut self, key: &str, value: &[u8]) -> Result<(), String>;
    fn secret_get(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, String>;
    /// Deleting a missing secret succeeds.
    fn secret_del(&mut self, key: &str) -> Result<(), String>;
}

/// Read and decode a JSON document. `None` when the key is absent.
pub fn get_json<T: DeserializeOwned>(
    store: &mut impl KvStore,
    key: &str,
    what: &str,
) -> Result<Option<T>, String> {
    match store.store_get(key, MAX_DOC_BYTES)? {
        None => Ok(None),
        Some(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|_| format!("stored {what} is unreadable")),
    }
}

/// Encode and store a JSON document (state namespace).
pub fn put_json<T: Serialize>(store: &mut impl KvStore, key: &str, value: &T, what: &str) -> Result<(), String> {
    let bytes = serde_json::to_vec(value).map_err(|_| format!("encode {what}"))?;
    store.store_put(key, &bytes)
}

/// Create a JSON document only if the key is absent.
pub fn put_new_json<T: Serialize>(
    store: &mut impl KvStore,
    key: &str,
    value: &T,
    what: &str,
) -> Result<PutNew, String> {
    let bytes = serde_json::to_vec(value).map_err(|_| format!("encode {what}"))?;
    store.store_put_new(key, &bytes)
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct LeaseBody {
    token: String,
    acquired_ms: u64,
}

/// A held lease. Release it with [`Lease::release`]; a lease that is never
/// released goes stale after [`LEASE_STALE_MS`].
#[derive(Debug, PartialEq, Eq)]
pub struct Lease {
    key: String,
    bytes: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum LeaseError {
    /// Another write holds a fresh lease.
    Busy,
    Store(String),
}

impl Lease {
    /// Take the lease at `key`: create it with `put_new`; if it exists and is
    /// stale (or unreadable), delete exactly the bytes seen and try once more.
    /// Polymarket's trade lock (`route/src/infra_parts/store.rs:14-60`).
    pub fn acquire(store: &mut impl KvStore, key: &str, token: [u8; 16], now_ms: u64) -> Result<Lease, LeaseError> {
        let body = LeaseBody { token: alloy_primitives::hex::encode(token), acquired_ms: now_ms };
        let bytes = serde_json::to_vec(&body).map_err(|_| LeaseError::Store("encode lease".into()))?;
        for attempt in 0..2 {
            match store.store_put_new(key, &bytes).map_err(LeaseError::Store)? {
                PutNew::Created => return Ok(Lease { key: key.to_owned(), bytes }),
                PutNew::Exists if attempt == 0 => {
                    let Some(current) = store.store_get(key, MAX_DOC_BYTES).map_err(LeaseError::Store)? else {
                        continue; // released in between
                    };
                    let stale = serde_json::from_slice::<LeaseBody>(&current)
                        .map(|held| now_ms.saturating_sub(held.acquired_ms) > LEASE_STALE_MS)
                        .unwrap_or(true);
                    if !stale {
                        return Err(LeaseError::Busy);
                    }
                    match store.store_del_if_value(key, &current).map_err(LeaseError::Store)? {
                        DelIfValue::Deleted | DelIfValue::Missing => continue,
                        DelIfValue::Changed => return Err(LeaseError::Busy),
                    }
                }
                PutNew::Exists => return Err(LeaseError::Busy),
            }
        }
        Err(LeaseError::Busy)
    }

    /// Delete the lease if it is still ours. A failure is ignored: the lease
    /// then goes stale.
    pub fn release(self, store: &mut impl KvStore) {
        let _ = store.store_del_if_value(&self.key, &self.bytes);
    }
}

#[cfg(test)]
pub(crate) mod fake {
    //! An in-memory store with the Bloom semantics the plan relies on
    //! (`private_store.rs:66-161`), plus failure injection.
    use std::collections::BTreeMap;

    use super::*;

    #[derive(Default, Debug)]
    pub struct MemStore {
        pub state: BTreeMap<String, Vec<u8>>,
        pub secrets: BTreeMap<String, Vec<u8>>,
        /// Mutating calls left before every mutation fails (crash injection).
        pub writes_left: Option<usize>,
    }

    impl MemStore {
        fn mutate(&mut self) -> Result<(), String> {
            match &mut self.writes_left {
                Some(0) => Err("injected store failure".into()),
                Some(n) => {
                    *n -= 1;
                    Ok(())
                }
                None => Ok(()),
            }
        }
    }

    impl KvStore for MemStore {
        fn store_get(&mut self, key: &str, max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
            assert!(!key.starts_with("creds/"), "state get of a secret key {key}");
            match self.state.get(key) {
                Some(bytes) if bytes.len() > max_bytes => Err("too large".into()),
                other => Ok(other.cloned()),
            }
        }
        fn store_put(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
            assert!(!key.starts_with("creds/"), "state put of a secret key {key}");
            self.mutate()?;
            self.state.insert(key.into(), value.to_vec());
            Ok(())
        }
        fn store_put_new(&mut self, key: &str, value: &[u8]) -> Result<PutNew, String> {
            assert!(!key.starts_with("creds/"));
            if self.state.contains_key(key) {
                return Ok(PutNew::Exists);
            }
            self.mutate()?;
            self.state.insert(key.into(), value.to_vec());
            Ok(PutNew::Created)
        }
        fn store_del_if_value(&mut self, key: &str, expected: &[u8]) -> Result<DelIfValue, String> {
            match self.state.get(key) {
                None => Ok(DelIfValue::Missing),
                Some(current) if current != expected => Ok(DelIfValue::Changed),
                Some(_) => {
                    self.mutate()?;
                    self.state.remove(key);
                    Ok(DelIfValue::Deleted)
                }
            }
        }
        fn store_list(&mut self, prefix: &str, _max_bytes: usize) -> Result<Vec<String>, String> {
            Ok(self.state.keys().filter(|key| key.starts_with(prefix)).cloned().collect())
        }
        fn secret_put(&mut self, key: &str, value: &[u8]) -> Result<(), String> {
            assert!(key.starts_with("creds/"), "secret outside creds/: {key}");
            self.mutate()?;
            self.secrets.insert(key.into(), value.to_vec());
            Ok(())
        }
        fn secret_get(&mut self, key: &str, _max_bytes: usize) -> Result<Option<Vec<u8>>, String> {
            assert!(key.starts_with("creds/"));
            Ok(self.secrets.get(key).cloned())
        }
        fn secret_del(&mut self, key: &str) -> Result<(), String> {
            assert!(key.starts_with("creds/"));
            self.mutate()?;
            self.secrets.remove(key);
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::fake::MemStore;
    use super::*;

    fn scope() -> Scope {
        Scope::new(Network::Testnet, "main", 0)
    }

    #[test]
    fn keys_follow_the_plan_layout() {
        let s = scope();
        assert_eq!(s.binding_key("v-1"), "votes/testnet/main/0/v-1/binding.json");
        assert_eq!(s.record_key("v-1"), "votes/testnet/main/0/v-1/record.json");
        assert_eq!(s.lease_key("v-1"), "votes/testnet/main/0/v-1/lease");
        assert_eq!(s.marker_key(), "markers/testnet/main/0/new.json");
        assert_eq!(s.signature_key("v-1"), "creds/votes/testnet/main/0/v-1/signature");
        assert_eq!(s.votes_dir(), "/petals/ballot/testnet/accounts/main/0/votes");
        let m = Scope::new(Network::Mainnet, "w", 3);
        assert_eq!(m.record_key("x"), "votes/mainnet/w/3/x/record.json");
        // Only the signature key is a secret-namespace (creds/) key.
        assert!(s.signature_key("v").starts_with("creds/"));
        for key in [s.binding_key("v"), s.record_key("v"), s.lease_key("v"), s.marker_key()] {
            assert!(!key.starts_with("creds"), "{key}");
        }
    }

    #[test]
    fn vote_ids_come_from_binding_or_record_keys() {
        let s = scope();
        let keys: Vec<String> = [
            "votes/testnet/main/0/b/record.json",
            "votes/testnet/main/0/a/binding.json",
            "votes/testnet/main/0/a/record.json",
            "votes/testnet/main/0/c/binding.json", // bound, record not yet written
            "votes/testnet/main/0/d/lease",
            "votes/testnet/main/0/Bad_ID/record.json",
            "votes/mainnet/main/0/z/record.json",
        ]
        .map(String::from)
        .to_vec();
        assert_eq!(s.vote_ids_from_keys(&keys), ["a", "b", "c"]);
    }

    #[test]
    fn json_helpers_round_trip_and_put_new_binds_once() {
        let mut store = MemStore::default();
        assert_eq!(get_json::<u32>(&mut store, "k", "x").unwrap(), None);
        put_json(&mut store, "k", &7_u32, "x").unwrap();
        assert_eq!(get_json::<u32>(&mut store, "k", "x").unwrap(), Some(7));
        assert_eq!(put_new_json(&mut store, "n", &1_u32, "x").unwrap(), PutNew::Created);
        assert_eq!(put_new_json(&mut store, "n", &2_u32, "x").unwrap(), PutNew::Exists);
        assert_eq!(get_json::<u32>(&mut store, "n", "x").unwrap(), Some(1), "put_new never overwrites");
        store.state.insert("bad".into(), b"{".to_vec());
        assert_eq!(get_json::<u32>(&mut store, "bad", "record").unwrap_err(), "stored record is unreadable");
    }

    #[test]
    fn lease_is_exclusive_until_released() {
        let mut store = MemStore::default();
        let key = scope().lease_key("v");
        let lease = Lease::acquire(&mut store, &key, [1; 16], 1_000).unwrap();
        assert_eq!(Lease::acquire(&mut store, &key, [2; 16], 1_001), Err(LeaseError::Busy));
        assert_eq!(
            Lease::acquire(&mut store, &key, [2; 16], 1_000 + LEASE_STALE_MS),
            Err(LeaseError::Busy),
            "exactly the stale age is still fresh"
        );
        lease.release(&mut store);
        assert!(store.state.is_empty());
        Lease::acquire(&mut store, &key, [2; 16], 1_002).unwrap();
    }

    #[test]
    fn stale_or_unreadable_lease_is_taken_over() {
        let mut store = MemStore::default();
        let key = scope().lease_key("v");
        let _abandoned = Lease::acquire(&mut store, &key, [1; 16], 1_000).unwrap();
        let taken = Lease::acquire(&mut store, &key, [2; 16], 1_001 + LEASE_STALE_MS).unwrap();
        let held: LeaseBody = serde_json::from_slice(&store.state[&key]).unwrap();
        assert_eq!(held.token, "02".repeat(16));
        // The old holder's release no longer deletes the new lease.
        _abandoned.release(&mut store);
        assert!(store.state.contains_key(&key));
        taken.release(&mut store);
        store.state.insert(key.clone(), b"garbage".to_vec());
        Lease::acquire(&mut store, &key, [3; 16], 5).unwrap();
    }

    #[test]
    fn lease_store_failure_is_reported() {
        let mut store = MemStore { writes_left: Some(0), ..MemStore::default() };
        assert_eq!(
            Lease::acquire(&mut store, "l", [1; 16], 1),
            Err(LeaseError::Store("injected store failure".into()))
        );
    }
}
