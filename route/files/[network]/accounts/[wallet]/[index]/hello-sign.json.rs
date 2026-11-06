// TEMPORARY Step 1 "hello signature" route: one exact snapshot.vote signature
// over a fixed test vote. Nothing is submitted. Delete after Step 1.
//
// Caps narrowed to the component's imports: bloom:sign (sign_payload),
// bloom:store (attempt record, state namespace) and bloom:vfs.read
// (WalletAccount::address_evm). Never pass approval_hint.
petal::route_file!(
    spec: petal::signing_write_spec("snapshot.vote")
        .caps(&["bloom:sign", "bloom:store", "bloom:vfs.read"]),
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
        crate::hello::read_hello(network, &account)
    },
    write: |ctx: &petal::Ctx, body: &[u8]| {
        let attempt = match crate::hello::Attempt::parse(body) {
            Ok(attempt) => attempt,
            Err(message) => return petal::error(-3, message),
        };
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
        crate::hello::sign_hello(ctx, network, &account, attempt)
    },
);
