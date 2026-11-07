//! Typed helpers shared by the ballot route components.
//!
//! This crate holds domain types and validators only. It never inspects route
//! identity and never selects behaviour from a path: each route file parses
//! its own parameters and calls these helpers with typed values.

use std::fmt;

pub mod hello;
pub mod snapshot;

/// Snapshot hub base URL for the testnet (confirmed in
/// docs/snapshot-testnet/FINDINGS.md §1).
pub const TESTNET_HUB: &str = "https://testnet.hub.snapshot.org";
/// Snapshot hub base URL for mainnet.
pub const MAINNET_HUB: &str = "https://hub.snapshot.org";

/// Maximum length of a Snapshot space id (ENS-style name).
pub const SPACE_MAX_LEN: usize = 64;
/// Maximum length of a client-chosen vote id.
pub const VOTE_ID_MAX_LEN: usize = 48;

/// The Snapshot environments this Petal may talk to. Every upstream URL is a
/// fixed `&'static str` chosen by variant, so callers cannot direct a request
/// at an arbitrary host.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Network {
    Testnet,
    Mainnet,
}

impl Network {
    pub const ALL: [Network; 2] = [Network::Testnet, Network::Mainnet];

    /// Parse the `[network]` route segment. Only exact lowercase names match.
    pub fn parse(value: &str) -> Result<Self, ValidationError> {
        match value {
            "testnet" => Ok(Network::Testnet),
            "mainnet" => Ok(Network::Mainnet),
            _ => Err(ValidationError::new("network must be testnet or mainnet")),
        }
    }

    /// The route segment for this network.
    pub fn as_str(self) -> &'static str {
        match self {
            Network::Testnet => "testnet",
            Network::Mainnet => "mainnet",
        }
    }

    /// Hub base URL (as reported in `status.json`).
    pub fn hub_url(self) -> &'static str {
        match self {
            Network::Testnet => TESTNET_HUB,
            Network::Mainnet => MAINNET_HUB,
        }
    }

    /// Hub GraphQL endpoint used for proposal reads.
    pub fn hub_graphql_url(self) -> &'static str {
        match self {
            Network::Testnet => "https://testnet.hub.snapshot.org/graphql",
            Network::Mainnet => "https://hub.snapshot.org/graphql",
        }
    }

    /// Sequencer endpoint that accepts signed vote envelopes.
    pub fn sequencer_url(self) -> &'static str {
        match self {
            Network::Testnet => "https://testnet.seq.snapshot.org",
            Network::Mainnet => "https://seq.snapshot.org",
        }
    }
}

/// A rejected input. The message is safe to return to the caller.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError(&'static str);

impl ValidationError {
    const fn new(message: &'static str) -> Self {
        Self(message)
    }

    pub fn message(&self) -> &'static str {
        self.0
    }
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.0)
    }
}

impl std::error::Error for ValidationError {}

fn is_label_byte(byte: u8) -> bool {
    byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-'
}

/// Validate a Snapshot space id: `^[a-z0-9-]+(\.[a-z0-9-]+)+$`, at most 64
/// bytes (for example `ens.eth` or `gitcoindao.eth`).
pub fn validate_space(value: &str) -> Result<&str, ValidationError> {
    if value.is_empty() || value.len() > SPACE_MAX_LEN {
        return Err(ValidationError::new("space must be 1-64 characters"));
    }
    let mut labels = 0usize;
    for label in value.split('.') {
        if label.is_empty() || !label.bytes().all(is_label_byte) {
            return Err(ValidationError::new(
                "space must be dot-separated labels of [a-z0-9-]",
            ));
        }
        labels += 1;
    }
    if labels < 2 {
        return Err(ValidationError::new(
            "space must contain at least one dot (e.g. name.eth)",
        ));
    }
    Ok(value)
}

/// Validate a Snapshot proposal id: `0x` followed by exactly 64 lowercase hex
/// digits.
pub fn validate_proposal_id(value: &str) -> Result<&str, ValidationError> {
    let Some(hex) = value.strip_prefix("0x") else {
        return Err(ValidationError::new("proposal id must start with 0x"));
    };
    if hex.len() != 64 {
        return Err(ValidationError::new(
            "proposal id must be 0x followed by 64 hex digits",
        ));
    }
    if !hex.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return Err(ValidationError::new(
            "proposal id must use lowercase hex digits",
        ));
    }
    Ok(value)
}

/// Validate a client-chosen vote id: `^[a-z0-9-]{1,48}$`.
pub fn validate_vote_id(value: &str) -> Result<&str, ValidationError> {
    if value.is_empty() || value.len() > VOTE_ID_MAX_LEN {
        return Err(ValidationError::new("vote id must be 1-48 characters"));
    }
    if !value.bytes().all(is_label_byte) {
        return Err(ValidationError::new("vote id must use only [a-z0-9-]"));
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn network_parse_accepts_known_names() {
        assert_eq!(Network::parse("testnet"), Ok(Network::Testnet));
        assert_eq!(Network::parse("mainnet"), Ok(Network::Mainnet));
        for network in Network::ALL {
            assert_eq!(Network::parse(network.as_str()), Ok(network));
        }
    }

    #[test]
    fn network_parse_rejects_others() {
        for bad in ["", "Testnet", "MAINNET", "goerli", "testnet/", " mainnet", "nonsense"] {
            assert!(Network::parse(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn network_urls_are_fixed_https_endpoints() {
        assert_eq!(Network::Mainnet.hub_url(), "https://hub.snapshot.org");
        assert_eq!(Network::Testnet.hub_url(), "https://testnet.hub.snapshot.org");
        assert_eq!(Network::Mainnet.hub_graphql_url(), "https://hub.snapshot.org/graphql");
        assert_eq!(
            Network::Testnet.hub_graphql_url(),
            "https://testnet.hub.snapshot.org/graphql"
        );
        assert_eq!(Network::Mainnet.sequencer_url(), "https://seq.snapshot.org");
        assert_eq!(Network::Testnet.sequencer_url(), "https://testnet.seq.snapshot.org");
        for network in Network::ALL {
            assert!(network.hub_graphql_url().starts_with(network.hub_url()));
            assert!(network.sequencer_url().starts_with("https://"));
        }
    }

    #[test]
    fn space_valid() {
        for ok in ["ens.eth", "gitcoindao.eth", "a.b", "my-dao.sub.eth", "0x1.eth"] {
            assert_eq!(validate_space(ok), Ok(ok));
        }
        let max = format!("{}.eth", "a".repeat(SPACE_MAX_LEN - 4));
        assert_eq!(max.len(), SPACE_MAX_LEN);
        assert!(validate_space(&max).is_ok());
    }

    #[test]
    fn space_invalid() {
        let too_long = format!("{}.eth", "a".repeat(SPACE_MAX_LEN - 3));
        for bad in [
            "", "ens", "ENS.eth", ".eth", "ens.", "ens..eth", "ens.eth/x", "ens_dao.eth",
            "ens eth", "../x.eth", too_long.as_str(),
        ] {
            assert!(validate_space(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn proposal_id_valid() {
        let id = format!("0x{}", "0123456789abcdef".repeat(4));
        assert_eq!(validate_proposal_id(&id), Ok(id.as_str()));
    }

    #[test]
    fn proposal_id_invalid() {
        let upper = format!("0x{}", "ABCDEF0123456789".repeat(4));
        let short = format!("0x{}", "a".repeat(63));
        let long = format!("0x{}", "a".repeat(65));
        let no_prefix = "a".repeat(66);
        let big_x = format!("0X{}", "a".repeat(64));
        let non_hex = format!("0x{}g", "a".repeat(63));
        for bad in ["", "0x", upper.as_str(), short.as_str(), long.as_str(),
                    no_prefix.as_str(), big_x.as_str(), non_hex.as_str()] {
            assert!(validate_proposal_id(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn vote_id_valid() {
        let max = "a".repeat(VOTE_ID_MAX_LEN);
        for ok in ["a", "vote-1", "0", "2026-11-06-ens", max.as_str()] {
            assert_eq!(validate_vote_id(ok), Ok(ok));
        }
    }

    #[test]
    fn vote_id_invalid() {
        let too_long = "a".repeat(VOTE_ID_MAX_LEN + 1);
        for bad in ["", "Vote", "vote_1", "vote.json", "a/b", "..", "vote 1", too_long.as_str()] {
            assert!(validate_vote_id(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn validation_error_displays_message() {
        let err = validate_vote_id("").unwrap_err();
        assert_eq!(err.to_string(), err.message());
    }
}
