//! Bounded GraphQL transport to the Snapshot hub.
//!
//! Every request goes to `Network::hub_graphql_url()`, a fixed `&'static str`
//! per network: callers choose neither host nor path. Query text is a
//! `&'static str` constant held by [`HubQuery`], and every caller-derived value
//! travels only in `variables`, never interpolated into the query.
//!
//! SDK surface used, exactly as the reference Petals call it at our pin
//! (`ba76944`, `crates/petal-sdk/src/lib.rs`):
//! - `petal::sdk::http_fetch(&HttpRequest, max_bytes) -> Result<HttpResponse, SdkError>`
//!   (lib.rs:405). It returns `HostStatus::BufferTooSmall` when the body
//!   exceeds `max_bytes`. Same call as bloom-petal-hyperliquid
//!   `route/src/workflow.rs:152` (POST JSON with a `content-type` header) and
//!   bloom-petal-polymarket `route/src/infra_parts/http.rs:25`.
//! - `petal::sdk::HttpRequest { method, url, headers, body }` (lib.rs:72) and
//!   `HttpResponse { status, headers, body }` (lib.rs:80).
//! - `petal::error(code, message)` (lib.rs:1677): `-4` is a backend failure.

use serde::Deserialize;
use serde::de::DeserializeOwned;
use serde_json::{Value, json};

use crate::Network;

/// Longest sanitized upstream message returned to a caller, in bytes.
pub const ERROR_MESSAGE_MAX: usize = 160;

/// One GraphQL operation: constant query text, its variables and the largest
/// response body accepted for it. Built only by typed constructors (see
/// `crate::proposals`), so a value can never reach the query text.
#[derive(Clone, Debug, PartialEq)]
pub struct HubQuery {
    query: &'static str,
    variables: Value,
    max_bytes: usize,
}

impl HubQuery {
    pub(crate) fn new(query: &'static str, variables: Value, max_bytes: usize) -> Self {
        Self { query, variables, max_bytes }
    }

    pub fn query(&self) -> &'static str {
        self.query
    }

    pub fn variables(&self) -> &Value {
        &self.variables
    }

    pub fn max_bytes(&self) -> usize {
        self.max_bytes
    }

    /// The JSON request body: `{"query": <constant>, "variables": {...}}`.
    pub fn body(&self) -> Vec<u8> {
        serde_json::to_vec(&json!({ "query": self.query, "variables": self.variables }))
            .unwrap_or_default()
    }

    /// The POST to this network's fixed hub GraphQL endpoint.
    pub fn request(&self, network: Network) -> petal::sdk::HttpRequest {
        petal::sdk::HttpRequest {
            method: "POST".into(),
            url: network.hub_graphql_url().into(),
            headers: vec![
                ("content-type".into(), "application/json".into()),
                ("accept".into(), "application/json".into()),
            ],
            body: self.body(),
        }
    }
}

/// A failed hub read. Every variant maps to `-4` with a sanitized message
/// that never contains a URL or more than [`ERROR_MESSAGE_MAX`] bytes of
/// upstream text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HubError {
    /// The host refused or failed the request (for example net policy).
    Transport(String),
    /// The response body exceeded the query's `max_bytes`.
    TooLarge,
    /// The hub answered with GraphQL `errors`.
    GraphQl(String),
    /// A non-2xx status without a GraphQL error message.
    Status(u16),
    /// The body was not the expected JSON shape.
    Decode,
}

impl HubError {
    fn from_sdk(error: &petal::SdkError) -> Self {
        match error {
            petal::SdkError::Host(petal::HostStatus::BufferTooSmall { .. }) => HubError::TooLarge,
            other => HubError::Transport(sanitize(&other.message())),
        }
    }

    pub fn message(&self, network: Network) -> String {
        let hub = network.as_str();
        match self {
            HubError::Transport(detail) => format!("snapshot {hub} hub request failed: {detail}"),
            HubError::TooLarge => format!("snapshot {hub} hub response is too large"),
            HubError::GraphQl(detail) => format!("snapshot {hub} hub error: {detail}"),
            HubError::Status(status) => format!("snapshot {hub} hub returned status {status}"),
            HubError::Decode => format!("snapshot {hub} hub returned an unexpected response"),
        }
    }

    pub fn response(&self, network: Network) -> petal::DispatchResponse {
        petal::error(-4, self.message(network))
    }
}

/// Keep upstream text safe to show: printable characters only, any token that
/// looks like a URL replaced, and at most [`ERROR_MESSAGE_MAX`] bytes (cut on
/// a UTF-8 boundary).
pub fn sanitize(text: &str) -> String {
    let words: Vec<&str> = text
        .split_whitespace()
        .map(|word| if word.contains("://") || word.contains("www.") { "[url]" } else { word })
        .collect();
    let joined: String = words.join(" ").chars().filter(|c| !c.is_control()).collect();
    let (cut, truncated) = truncate_utf8(&joined, ERROR_MESSAGE_MAX);
    if truncated { format!("{cut}…") } else { cut.to_owned() }
}

/// Cut `text` to at most `max` bytes on a UTF-8 character boundary. Returns
/// the prefix and whether anything was removed.
pub fn truncate_utf8(text: &str, max: usize) -> (&str, bool) {
    if text.len() <= max {
        return (text, false);
    }
    let mut end = max;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    (&text[..end], true)
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: Option<T>,
    #[serde(default)]
    errors: Option<Vec<GraphQlError>>,
}

#[derive(Deserialize)]
struct GraphQlError {
    #[serde(default)]
    message: String,
}

#[derive(Deserialize)]
struct ErrorsOnly {
    #[serde(default)]
    errors: Option<Vec<GraphQlError>>,
}

fn first_error(errors: Option<Vec<GraphQlError>>) -> Option<HubError> {
    let errors = errors?;
    let first = errors.into_iter().next()?;
    let message = if first.message.is_empty() { "unknown error".into() } else { sanitize(&first.message) };
    Some(HubError::GraphQl(message))
}

/// Interpret a hub response: size bound first, then GraphQL `errors`
/// (the hub sends them with 200, 400 or 500), then the status, then `data`.
pub fn parse_response<T: DeserializeOwned>(status: u16, body: &[u8], max_bytes: usize) -> Result<T, HubError> {
    if body.len() > max_bytes {
        return Err(HubError::TooLarge);
    }
    let ok = (200..300).contains(&status);
    match serde_json::from_slice::<Envelope<T>>(body) {
        Ok(envelope) => {
            if let Some(error) = first_error(envelope.errors) {
                return Err(error);
            }
            if !ok {
                return Err(HubError::Status(status));
            }
            envelope.data.ok_or(HubError::Decode)
        }
        Err(_) => {
            // The data did not match T; still prefer a GraphQL error message.
            if let Ok(errors) = serde_json::from_slice::<ErrorsOnly>(body)
                && let Some(error) = first_error(errors.errors)
            {
                return Err(error);
            }
            Err(if ok { HubError::Decode } else { HubError::Status(status) })
        }
    }
}

/// POST `query` to `network`'s hub and decode its `data`.
pub fn fetch<T: DeserializeOwned>(network: Network, query: &HubQuery) -> Result<T, HubError> {
    let response = petal::sdk::http_fetch(&query.request(network), query.max_bytes)
        .map_err(|error| HubError::from_sdk(&error))?;
    parse_response(response.status, &response.body, query.max_bytes)
}

/// Seconds since the Unix epoch from the host clock, if it is available
/// (`petal::sdk::try_now_ms`, lib.rs:650).
pub fn now_secs() -> Option<u64> {
    petal::sdk::try_now_ms().ok().map(|ms| ms / 1000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Deserialize, PartialEq)]
    struct Data {
        value: u32,
    }

    #[test]
    fn request_targets_only_the_fixed_hub_endpoint() {
        let query = HubQuery::new("query Q($x: String!) { q(x: $x) }", json!({"x": "a\"b"}), 10);
        for network in Network::ALL {
            let request = query.request(network);
            assert_eq!(request.method, "POST");
            assert_eq!(request.url, network.hub_graphql_url());
            assert!(request.headers.contains(&("content-type".into(), "application/json".into())));
            let body: Value = serde_json::from_slice(&request.body).unwrap();
            assert_eq!(body["query"], "query Q($x: String!) { q(x: $x) }");
            assert_eq!(body["variables"], json!({"x": "a\"b"}));
        }
    }

    #[test]
    fn parse_accepts_data() {
        assert_eq!(parse_response::<Data>(200, br#"{"data":{"value":7}}"#, 100), Ok(Data { value: 7 }));
    }

    #[test]
    fn graphql_errors_map_to_a_sanitized_backend_error() {
        let body = include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../docs/snapshot-testnet/responses/ballot_graphql_error_testnet.json"
        ));
        let error = parse_response::<Data>(400, body, 1_000).unwrap_err();
        assert_eq!(
            error,
            HubError::GraphQl("Cannot query field \"nope\" on type \"Query\". Did you mean \"vote\"?".into())
        );
        // Errors win even with a 200 status, and with data present.
        let both = br#"{"data":{"value":1},"errors":[{"message":"boom"}]}"#;
        assert_eq!(parse_response::<Data>(200, both, 1_000), Err(HubError::GraphQl("boom".into())));
        let response = error.response(Network::Testnet);
        match response {
            petal::DispatchResponse::Error { code, message } => {
                assert_eq!(code, -4);
                assert!(message.starts_with("snapshot testnet hub error: Cannot query field"));
                assert!(!message.contains("://"));
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn status_and_decode_failures() {
        assert_eq!(parse_response::<Data>(502, b"<html>bad gateway</html>", 1_000), Err(HubError::Status(502)));
        assert_eq!(parse_response::<Data>(500, br#"{"data":null}"#, 1_000), Err(HubError::Status(500)));
        assert_eq!(parse_response::<Data>(200, b"not json", 1_000), Err(HubError::Decode));
        assert_eq!(parse_response::<Data>(200, br#"{"data":null}"#, 1_000), Err(HubError::Decode));
        assert_eq!(parse_response::<Data>(200, br#"{"data":{"other":1}}"#, 1_000), Err(HubError::Decode));
    }

    #[test]
    fn oversized_response_is_rejected_before_parsing() {
        let body = br#"{"data":{"value":7}}"#;
        assert_eq!(parse_response::<Data>(200, body, body.len() - 1), Err(HubError::TooLarge));
        assert!(parse_response::<Data>(200, body, body.len()).is_ok());
        assert_eq!(
            HubError::from_sdk(&petal::SdkError::Host(petal::HostStatus::BufferTooSmall { needed: 9 })),
            HubError::TooLarge
        );
        assert_eq!(HubError::TooLarge.message(Network::Mainnet), "snapshot mainnet hub response is too large");
    }

    #[test]
    fn transport_errors_are_sanitized() {
        let denied = HubError::from_sdk(&petal::SdkError::Host(petal::HostStatus::Denied));
        assert_eq!(denied.message(Network::Testnet), "snapshot testnet hub request failed: denied");
        let noisy = petal::SdkError::Message(format!(
            "connect https://hub.snapshot.org/graphql?x=1 failed\n{}",
            "x".repeat(500)
        ));
        let HubError::Transport(detail) = HubError::from_sdk(&noisy) else { panic!() };
        assert!(!detail.contains("://") && !detail.contains("?x=1"));
        assert!(!detail.contains('\n'));
        assert!(detail.len() <= ERROR_MESSAGE_MAX + '…'.len_utf8());
    }

    #[test]
    fn sanitize_bounds_and_strips() {
        assert_eq!(sanitize("a  b\tc"), "a b c");
        assert_eq!(sanitize("see www.example.com now"), "see [url] now");
        let long = "é".repeat(200);
        let cut = sanitize(&long);
        assert!(cut.ends_with('…'));
        assert!(cut.len() <= ERROR_MESSAGE_MAX + '…'.len_utf8());
    }

    #[test]
    fn truncate_utf8_never_splits_a_character() {
        assert_eq!(truncate_utf8("abc", 3), ("abc", false));
        assert_eq!(truncate_utf8("abc", 2), ("ab", true));
        // "é" is 2 bytes, "提" is 3: cutting inside either backs off.
        assert_eq!(truncate_utf8("aé", 2), ("a", true));
        assert_eq!(truncate_utf8("提案", 4), ("提", true));
        assert_eq!(truncate_utf8("提案", 5), ("提", true));
        assert_eq!(truncate_utf8("提案", 6), ("提案", false));
        assert_eq!(truncate_utf8("😀", 3), ("", true));
    }
}
