//! Snapshot `Vote` EIP-712 typed data, exactly as current snapshot.js builds it.
//!
//! Source of truth: `~/work/step0/FINDINGS.md` §2 and §6.
//! - Domain `{name:"snapshot", version:"0.1.4"}`, with no chainId and no
//!   verifyingContract (snapshot.js `src/sign/index.ts:40-51`).
//! - `Vote(string from,string space,uint64 timestamp,string proposal,uint32 choice,string reason,string app,string metadata)`
//!   (snapshot.js `src/sign/types.ts:204-215`). `from` and `proposal` are
//!   **strings**, hashed as UTF-8 text even though they look like hex.
//! - `from` is the EIP-55 checksummed address (snapshot.js `sign()`).
//!
//! Every function here is pure, so the whole EIP-712 pipeline is host-tested
//! against the golden vector.

use alloy_primitives::{Address, B256, Signature, keccak256};
use serde_json::{Value, json};
use sha2::{Digest as _, Sha256};

pub const DOMAIN_NAME: &str = "snapshot";
pub const DOMAIN_VERSION: &str = "0.1.4";
pub const DOMAIN_TYPE: &str = "EIP712Domain(string name,string version)";
pub const VOTE_TYPE: &str = "Vote(string from,string space,uint64 timestamp,string proposal,uint32 choice,string reason,string app,string metadata)";
pub const PRIMARY_TYPE: &str = "Vote";
/// `0x19 0x01 ‖ domainSeparator ‖ structHash`.
pub const PREIMAGE_LEN: usize = 66;

/// A single-choice/basic Snapshot vote (`voteTypes`, uint32 choice).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Vote {
    /// EIP-55 checksummed address, exactly as signed.
    pub from: String,
    pub space: String,
    pub timestamp: u64,
    /// `0x…` proposal id, signed as a string.
    pub proposal: String,
    pub choice: u32,
    pub reason: String,
    pub app: String,
    pub metadata: String,
}

/// EIP-712 `string` encoding: keccak256 of the UTF-8 bytes, never hex-decoded.
pub fn hash_string(value: &str) -> B256 {
    keccak256(value.as_bytes())
}

fn word_u64(value: u64) -> [u8; 32] {
    let mut word = [0_u8; 32];
    word[24..].copy_from_slice(&value.to_be_bytes());
    word
}

pub fn domain_type_hash() -> B256 {
    keccak256(DOMAIN_TYPE.as_bytes())
}

pub fn vote_type_hash() -> B256 {
    keccak256(VOTE_TYPE.as_bytes())
}

pub fn domain_separator() -> B256 {
    let mut encoded = Vec::with_capacity(32 * 3);
    encoded.extend_from_slice(domain_type_hash().as_slice());
    encoded.extend_from_slice(hash_string(DOMAIN_NAME).as_slice());
    encoded.extend_from_slice(hash_string(DOMAIN_VERSION).as_slice());
    keccak256(&encoded)
}

impl Vote {
    pub fn struct_hash(&self) -> B256 {
        let mut encoded = Vec::with_capacity(32 * 9);
        encoded.extend_from_slice(vote_type_hash().as_slice());
        encoded.extend_from_slice(hash_string(&self.from).as_slice());
        encoded.extend_from_slice(hash_string(&self.space).as_slice());
        encoded.extend_from_slice(&word_u64(self.timestamp));
        encoded.extend_from_slice(hash_string(&self.proposal).as_slice());
        encoded.extend_from_slice(&word_u64(u64::from(self.choice)));
        encoded.extend_from_slice(hash_string(&self.reason).as_slice());
        encoded.extend_from_slice(hash_string(&self.app).as_slice());
        encoded.extend_from_slice(hash_string(&self.metadata).as_slice());
        keccak256(&encoded)
    }

    /// The exact bytes Bloom signs: `0x19 0x01 ‖ domainSeparator ‖ structHash`.
    pub fn preimage(&self) -> [u8; PREIMAGE_LEN] {
        let mut preimage = [0_u8; PREIMAGE_LEN];
        preimage[0] = 0x19;
        preimage[1] = 0x01;
        preimage[2..34].copy_from_slice(domain_separator().as_slice());
        preimage[34..].copy_from_slice(self.struct_hash().as_slice());
        preimage
    }

    /// The EIP-712 digest, `keccak256(preimage)`: the sign request's claimed hash.
    pub fn digest(&self) -> B256 {
        keccak256(self.preimage())
    }

    /// The full EIP-712 document (with `EIP712Domain` and `primaryType`), in
    /// the shape Bloom's `/tools/eip712/hash` and `cast` accept. It is public:
    /// it contains no secret.
    pub fn typed_data(&self) -> Value {
        json!({
            "domain": {"name": DOMAIN_NAME, "version": DOMAIN_VERSION},
            "types": {
                "EIP712Domain": [
                    {"name": "name", "type": "string"},
                    {"name": "version", "type": "string"}
                ],
                "Vote": [
                    {"name": "from", "type": "string"},
                    {"name": "space", "type": "string"},
                    {"name": "timestamp", "type": "uint64"},
                    {"name": "proposal", "type": "string"},
                    {"name": "choice", "type": "uint32"},
                    {"name": "reason", "type": "string"},
                    {"name": "app", "type": "string"},
                    {"name": "metadata", "type": "string"}
                ]
            },
            "primaryType": PRIMARY_TYPE,
            "message": {
                "from": self.from,
                "space": self.space,
                "timestamp": self.timestamp,
                "proposal": self.proposal,
                "choice": self.choice,
                "reason": self.reason,
                "app": self.app,
                "metadata": self.metadata
            }
        })
    }
}

/// Parse a 20-byte EVM address in any letter case and return its EIP-55
/// checksummed form, which is what snapshot.js signs as `from`.
pub fn checksum_address(value: &str) -> Result<String, &'static str> {
    let trimmed = value.trim();
    let hex = trimmed
        .strip_prefix("0x")
        .ok_or("address must start with 0x")?;
    if hex.len() != 40 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err("address must be 0x followed by 40 hex digits");
    }
    let address: Address = trimmed.parse().map_err(|_| "address is not valid hex")?;
    Ok(address.to_checksum(None))
}

/// Recover the signer of a 65-byte recoverable secp256k1 signature over a
/// prehashed digest. Mirrors bloom-petal-hyperliquid `protocol.rs:480-486`.
/// The signature bytes are only read, never copied into the result or an error.
pub fn recover_signer(digest: &B256, signature: &[u8]) -> Result<Address, &'static str> {
    if signature.len() != 65 {
        return Err("signature must be 65 bytes");
    }
    let signature = Signature::from_raw(signature).map_err(|_| "signature is malformed")?;
    signature
        .recover_address_from_prehash(digest)
        .map_err(|_| "signature does not recover to an address")
}

/// Public fingerprint of signature bytes: lowercase hex sha256. This is what
/// may be recorded instead of the confidential bytes themselves.
pub fn signature_fingerprint(signature: &[u8]) -> String {
    alloy_primitives::hex::encode(Sha256::digest(signature))
}

/// `0x`-prefixed lowercase hex of a 32-byte value.
pub fn hex32(value: &B256) -> String {
    format!("{value:#x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    // ~/work/step0/FINDINGS.md §6 and golden/{message,digest_parts}.txt.
    const GOLDEN_FROM: &str = "0x042abe4EE65cBC71Bd11297DCea2d69e1650a596";
    const GOLDEN_PROPOSAL: &str =
        "0x3b1261975f64a6e11b3f365280580fbe0e7e7589799f122f4c0bcd61021bdcc6";
    const GOLDEN_TYPE_HASH: &str =
        "0x7bf372d792402c0796ec3567238391d04616792abb3fe96c99adc02d8b8cdffa";
    const GOLDEN_DOMAIN_SEPARATOR: &str =
        "0x484fce18f892e8535a4b6700e197a8026f4213f809d23ae117da03b497e18670";
    const GOLDEN_STRUCT_HASH: &str =
        "0x2b0bbfd2824580ad8696e79f475d4cd35594bcafb3fafac700bc50bd97f65f08";
    const GOLDEN_DIGEST: &str =
        "0x9431fb703c91b4a1c216343688a73580e102f80585c4511376f7b58281ff0251";
    /// `golden/typed_data.json`, verbatim.
    const GOLDEN_TYPED_DATA: &str = r#"{"domain":{"name":"snapshot","version":"0.1.4"},"types":{"EIP712Domain":[{"name":"name","type":"string"},{"name":"version","type":"string"}],"Vote":[{"name":"from","type":"string"},{"name":"space","type":"string"},{"name":"timestamp","type":"uint64"},{"name":"proposal","type":"string"},{"name":"choice","type":"uint32"},{"name":"reason","type":"string"},{"name":"app","type":"string"},{"name":"metadata","type":"string"}]},"message":{"from":"0x042abe4EE65cBC71Bd11297DCea2d69e1650a596","space":"bloom-ballot-test.eth","timestamp":1793923200,"proposal":"0x3b1261975f64a6e11b3f365280580fbe0e7e7589799f122f4c0bcd61021bdcc6","choice":1,"reason":"","app":"bloom-ballot","metadata":"{}"},"primaryType":"Vote"}"#;

    fn golden_vote() -> Vote {
        Vote {
            from: GOLDEN_FROM.into(),
            space: "bloom-ballot-test.eth".into(),
            timestamp: 1_793_923_200,
            proposal: GOLDEN_PROPOSAL.into(),
            choice: 1,
            reason: String::new(),
            app: "bloom-ballot".into(),
            metadata: "{}".into(),
        }
    }

    // (a) The golden vector reproduces every intermediate, the preimage and
    // the digest that Bloom's /tools/eip712/hash and the independent cast
    // computation agree on.
    #[test]
    fn golden_vector_reproduces_digest_and_preimage() {
        let vote = golden_vote();
        assert_eq!(hex32(&vote_type_hash()), GOLDEN_TYPE_HASH);
        assert_eq!(hex32(&domain_separator()), GOLDEN_DOMAIN_SEPARATOR);
        assert_eq!(hex32(&vote.struct_hash()), GOLDEN_STRUCT_HASH);
        let expected_preimage = format!(
            "0x1901{}{}",
            &GOLDEN_DOMAIN_SEPARATOR[2..],
            &GOLDEN_STRUCT_HASH[2..]
        );
        let preimage = vote.preimage();
        assert_eq!(preimage.len(), PREIMAGE_LEN);
        assert_eq!(
            format!("0x{}", alloy_primitives::hex::encode(preimage)),
            expected_preimage
        );
        assert_eq!(hex32(&vote.digest()), GOLDEN_DIGEST);
        assert_eq!(hex32(&keccak256(preimage)), GOLDEN_DIGEST);
    }

    #[test]
    fn golden_vector_typed_data_matches_the_golden_document() {
        let golden: Value = serde_json::from_str(GOLDEN_TYPED_DATA).unwrap();
        assert_eq!(golden_vote().typed_data(), golden);
    }

    // (b) `from` and `proposal` look like hex but are EIP-712 strings: they are
    // hashed as their UTF-8 text. Hex-decoding them (cast keccak "0x…") gives a
    // different, wrong digest.
    #[test]
    fn string_fields_are_hashed_as_utf8_text_not_hex_bytes() {
        let proposal_text = hash_string(GOLDEN_PROPOSAL);
        let proposal_bytes = alloy_primitives::hex::decode(GOLDEN_PROPOSAL).unwrap();
        assert_eq!(proposal_text, keccak256(GOLDEN_PROPOSAL.as_bytes()));
        assert_ne!(proposal_text, keccak256(&proposal_bytes));
        let from_bytes = alloy_primitives::hex::decode(GOLDEN_FROM).unwrap();
        assert_ne!(hash_string(GOLDEN_FROM), keccak256(&from_bytes));

        // Re-encode the struct with hex-decoded strings: the digest must differ.
        let vote = golden_vote();
        let mut encoded = Vec::new();
        encoded.extend_from_slice(vote_type_hash().as_slice());
        encoded.extend_from_slice(keccak256(&from_bytes).as_slice());
        encoded.extend_from_slice(hash_string(&vote.space).as_slice());
        encoded.extend_from_slice(&word_u64(vote.timestamp));
        encoded.extend_from_slice(keccak256(&proposal_bytes).as_slice());
        encoded.extend_from_slice(&word_u64(u64::from(vote.choice)));
        encoded.extend_from_slice(hash_string(&vote.reason).as_slice());
        encoded.extend_from_slice(hash_string(&vote.app).as_slice());
        encoded.extend_from_slice(hash_string(&vote.metadata).as_slice());
        assert_ne!(keccak256(&encoded), vote.struct_hash());
    }

    // (c) snapshot.js signs the checksummed address. A lowercase `from` is a
    // different string and therefore a different digest.
    #[test]
    fn lowercase_from_changes_the_digest_and_checksum_restores_it() {
        let mut lower = golden_vote();
        lower.from = GOLDEN_FROM.to_ascii_lowercase();
        assert_ne!(hex32(&lower.digest()), GOLDEN_DIGEST);
        assert_eq!(checksum_address(&lower.from).unwrap(), GOLDEN_FROM);
        assert_eq!(
            checksum_address(&GOLDEN_FROM.to_ascii_uppercase().replacen("0X", "0x", 1)).unwrap(),
            GOLDEN_FROM
        );
        lower.from = checksum_address(&lower.from).unwrap();
        assert_eq!(hex32(&lower.digest()), GOLDEN_DIGEST);
    }

    #[test]
    fn checksum_address_rejects_malformed_input() {
        for bad in ["", "0x", "042abe4EE65cBC71Bd11297DCea2d69e1650a596", "0x1234", "0xZZ2abe4EE65cBC71Bd11297DCea2d69e1650a596"] {
            assert!(checksum_address(bad).is_err(), "{bad:?} should be rejected");
        }
    }

    #[test]
    fn every_vote_field_changes_the_digest() {
        let base = golden_vote().digest();
        let mut variants = Vec::new();
        let mut v = golden_vote(); v.space.push('x'); variants.push(v);
        let mut v = golden_vote(); v.timestamp += 1; variants.push(v);
        let mut v = golden_vote(); v.proposal.replace_range(2..3, "4"); variants.push(v);
        let mut v = golden_vote(); v.choice = 2; variants.push(v);
        let mut v = golden_vote(); v.reason = "x".into(); variants.push(v);
        let mut v = golden_vote(); v.app = "other".into(); variants.push(v);
        let mut v = golden_vote(); v.metadata = String::new(); variants.push(v);
        for variant in variants {
            assert_ne!(variant.digest(), base, "{variant:?}");
        }
    }

    /// Deterministic test key, as in bloom-petal-hyperliquid `protocol.rs:670-690`.
    fn sign_with_test_key(digest: &B256) -> ([u8; 65], Address) {
        use k256::ecdsa::SigningKey;
        let key = SigningKey::from_bytes(&[0x11; 32].into()).expect("valid key");
        let (signature, recovery_id) = key.sign_prehash_recoverable(digest.as_slice()).unwrap();
        let mut raw = [0_u8; 65];
        raw[..64].copy_from_slice(&signature.to_bytes());
        raw[64] = recovery_id.to_byte();
        let point = key.verifying_key().to_encoded_point(false);
        let address = Address::from_slice(&keccak256(&point.as_bytes()[1..])[12..]);
        (raw, address)
    }

    // (d) The recovery helper, against a signature produced at test time over
    // the golden digest (hermetic: no signature bytes are stored).
    #[test]
    fn recover_signer_recovers_a_freshly_signed_golden_digest() {
        let digest = golden_vote().digest();
        let (raw, expected) = sign_with_test_key(&digest);
        assert!(recover_signer(&digest, &raw) == Ok(expected));
        let mut legacy_v = raw;
        legacy_v[64] += 27;
        assert!(recover_signer(&digest, &legacy_v) == Ok(expected), "v = 27/28 also recovers");
        let other = golden_vote().struct_hash();
        assert!(recover_signer(&other, &raw) != Ok(expected), "another digest recovers elsewhere");
        assert!(recover_signer(&digest, &raw[..64]).is_err());
    }

    // (d) The golden vector's own throwaway signature recovers the throwaway
    // address. The signature is read at test time from step0's golden files so
    // that no signature bytes live in this repository; it is never printed.
    #[test]
    fn recover_signer_recovers_the_golden_throwaway_address() {
        let path = std::env::var("BALLOT_GOLDEN_SIGNATURE").unwrap_or_else(|_| {
            format!(
                "{}/work/step0/golden/signature.hex",
                std::env::var("HOME").unwrap_or_default()
            )
        });
        let Ok(text) = std::fs::read_to_string(&path) else {
            eprintln!("SKIPPED golden throwaway recovery: {path} not found (set BALLOT_GOLDEN_SIGNATURE)");
            return;
        };
        let raw = alloy_primitives::hex::decode(text.trim()).expect("golden signature is hex");
        let digest = golden_vote().digest();
        let recovered = recover_signer(&digest, &raw);
        let expected: Address = GOLDEN_FROM.parse().unwrap();
        assert!(recovered == Ok(expected), "golden signature must recover the throwaway signer");
        assert_eq!(recovered.unwrap().to_checksum(None), GOLDEN_FROM);
    }

    #[test]
    fn signature_fingerprint_is_sha256_hex() {
        assert_eq!(
            signature_fingerprint(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(signature_fingerprint(&[0_u8; 65]).len(), 64);
    }
}
