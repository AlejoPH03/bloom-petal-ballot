// One Snapshot proposal by id. One POST to the network's fixed hub GraphQL
// endpoint; the id travels only in GraphQL variables. Times are the hub's.
// An id the hub does not know is "not found" (-1). Imports bloom:http only.
petal::route_file!(
    spec: petal::http_read_spec(30_000),
    read: |ctx: &petal::Ctx| {
        let network = match petal::param(ctx, "network").and_then(|value| {
            crate::Network::parse(value).map_err(|error| petal::error(-1, error.message()))
        }) {
            Ok(network) => network,
            Err(response) => return response,
        };
        let id = match petal::param(ctx, "proposal") {
            Ok(id) => id,
            Err(response) => return response,
        };
        let query = match crate::proposals::proposal_query(id) {
            Ok(query) => query,
            Err(error) => return petal::error(-3, error.message()),
        };
        let data: crate::proposals::ProposalData = match crate::hub::fetch(network, &query) {
            Ok(data) => data,
            Err(error) => return error.response(network),
        };
        let Some(proposal) = data.proposal else {
            return petal::error(-1, "proposal not found");
        };
        petal::read_json_value(&crate::proposals::project_proposal(
            network,
            proposal,
            crate::hub::now_secs(),
        ))
    },
);
