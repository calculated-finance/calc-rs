# CALC Node Interface

Shared protocol between CALC strategies and approved external singleton node contracts.

Singleton storage keys node instances by `(strategy address, strategy revision, node index)`. Sender strategy registers opaque configuration, executes or cancels node, then commits after generated messages where required by strategy engine.

## Execute messages

- `Register { strategy_revision, node_index, config }`
- `Execute { strategy_revision, node_index }`
- `Commit { strategy_revision, node_index }`
- `Cancel { strategy_revision, node_index }`

`Execute` and `Cancel` return JSON-encoded `Vec<CosmosMsg>` in contract response data. Strategy reads the modern `MsgExecuteContractResponse` reply first and retains a legacy reply-data fallback. Strategy executes returned messages as itself.

The strategy engine calls `Commit` after a failed `Execute` or `Cancel`, matching its existing local-node error path, and after non-empty returned message sequences. It does not call `Commit` after a successful zero-message response. Implementations must make `Commit` safe and idempotent when no execution state was staged.

## Queries

- `Details` returns reusable opaque registration configuration.
- `IsSatisfied` returns condition result.
- `Balances` returns balances owned or reserved by node.

Interface deliberately omits node kind, affiliates, owner, strategy status, and interface-version metadata. Manager registry controls approved address, code ID, checksum, status, and size weight.

## Implementation requirements

Approved singleton implementations must:

- store the deploying manager address and reject `Register` unless `info.sender` exists in that manager's strategy registry;
- key all later state access by `info.sender`, strategy revision, and node index so one strategy cannot operate another strategy's records;
- validate opaque configuration during `Register`;
- avoid custody of strategy funds that would require access to an unreferenced old revision after a failed `Cancel`;
- avoid affiliate-bearing distribution or withdrawal behavior. Distribution remains an internal strategy action because external nodes deliberately receive no owner or affiliate context.

These rules are enforced through manager checksum approval and node-repository review. Shared message types cannot enforce another contract's authorization or fund-flow implementation.
