// The owner's standing rules for this account's votes (ADVISORY: the agent
// can edit it; every vote still needs the owner's approval). Read: the
// mandate, notes, updated_ms, the advisory note and the last-write marker.
// Write: the whole mandate ({} clears it). docs/plan-mandate-inbox.md.
//
// Caps = compiled imports: bloom:store only (write_spec narrowed; no sign,
// no http, no vfs). The clock is bloom:env/runtime, which needs no cap.
petal::route_file!(
    spec: petal::write_spec().caps(&["bloom:store"]),
    read: |ctx: &petal::Ctx| {
        let network = match petal::param(ctx, "network").and_then(|value| {
            crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))
        }) {
            Ok(network) => network,
            Err(response) => return response,
        };
        let account = match petal::wallet_account(ctx) {
            Ok(account) => account,
            Err(response) => return response,
        };
        let scope = crate::votes::store::Scope::new(network, account.wallet(), account.number());
        let mut host = crate::votes::host::SdkHost::new(ctx, &account);
        match crate::mandate::read_document(&mut host, &scope) {
            Ok(document) => petal::read_json_value(&document),
            Err(error) => petal::error(-4, format!("read mandate: {error}")),
        }
    },
    write: |ctx: &petal::Ctx, body: &[u8]| {
        let network = match petal::param(ctx, "network").and_then(|value| {
            crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))
        }) {
            Ok(network) => network,
            Err(response) => return response,
        };
        let account = match petal::wallet_account(ctx) {
            Ok(account) => account,
            Err(response) => return response,
        };
        let scope = crate::votes::store::Scope::new(network, account.wallet(), account.number());
        let mut host = crate::votes::host::SdkHost::new(ctx, &account);
        let now_ms = crate::votes::flow::VoteHost::now_ms(&mut host);
        match crate::mandate::write(&mut host, &scope, now_ms, body) {
            (0, _) => petal::DispatchResponse::Write,
            (code, message) => petal::error(code, message),
        }
    },
);
