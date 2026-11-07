// Cast one Snapshot vote (docs/plan-votes.md §3). Write body
// {vote_id, space, proposal, choice, reason?}; read: body schema, rules and
// the last-write marker. Every outcome is recorded in the marker and in
// votes/<vote_id>.json, because a mounted write runs detached.
//
// Caps = compiled imports: bloom:http (hub checks, sequencer POST),
// bloom:sign (sign_payload), bloom:store (binding, record, marker, lease, the
// creds/ signature), bloom:vfs.read (WalletAccount::address_evm). No
// approval_hint is ever passed.
petal::route_file!(
    spec: petal::signing_write_spec("snapshot.vote")
        .caps(&["bloom:http", "bloom:sign", "bloom:store", "bloom:vfs.read"]),
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
        match crate::votes::flow::new_document(&mut host, &scope) {
            Ok(document) => petal::read_json_value(&document),
            Err(error) => petal::error(-4, format!("read last write: {error}")),
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
        let outcome = crate::votes::flow::run_write(&mut host, &scope, body);
        // -3, never -2, for refusals and pending approvals: Bloom drops the
        // message of a denial (SKILL.md:517-523).
        if outcome.code == 0 {
            petal::DispatchResponse::Write
        } else {
            petal::error(outcome.code, outcome.message)
        }
    },
);
