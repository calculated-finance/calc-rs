# CALC Node Interface

Shared protocol between CALC strategies and approved external singleton node contracts.

Singleton storage keys node instances by `(strategy address, strategy revision, node index)`. Sender strategy registers opaque configuration, executes or cancels node, then commits after generated messages where required by strategy engine.

## Execute messages

- `Register { strategy_revision, node_index, config }`
- `Execute { strategy_revision, node_index }`
- `Commit { strategy_revision, node_index }`
- `Cancel { strategy_revision, node_index }`

`Execute` and `Cancel` return JSON-encoded `Vec<CosmosMsg>` in contract response data. Strategy executes those messages as itself.

## Queries

- `Details` returns reusable opaque registration configuration.
- `IsSatisfied` returns condition result.
- `Balances` returns balances owned or reserved by node.

Interface deliberately omits node kind, affiliates, owner, strategy status, and interface-version metadata. Manager registry controls approved address, code ID, checksum, status, and size weight.
