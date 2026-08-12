# Scheduler Contract

The scheduler stores off-chain CALC triggers for block-height and timestamp conditions. Time and cron schedules resolve to timestamp triggers. Keepers execute satisfied triggers and direct every coin escrowed on that trigger to themselves or a nominated rebate receiver.

## Instantiate and migrate

New schedulers instantiate with an explicit immutable owner:

```rust
SchedulerInstantiateMsg {
    owner: Addr,
}
```

New instances start with rebate enforcement disabled and no accepted minimums. Existing deployments initialize their first config during migration:

```rust
MigrateMsg {
    owner: Addr,
    enforcement_enabled: bool,
    accepted_rebate_minimums: Vec<Coin>,
}
```

Migration authorization is provided by the chain's contract-admin mechanism.

## Rebate policy config

`SchedulerQueryMsg::Config {}` returns:

```rust
SchedulerConfig {
    owner: Addr,
    enforcement_enabled: bool,
    accepted_rebate_minimums: Vec<Coin>,
}
```

Only the stored owner can replace policy through one atomic update:

```rust
SchedulerExecuteMsg::UpdateConfig {
    enforcement_enabled: bool,
    accepted_rebate_minimums: Vec<Coin>,
}
```

The update replaces the entire minimum list and enforcement flag. It cannot attach funds. Owner remains unchanged. Minimum lists reject zero amounts and duplicate denoms. Enabled enforcement requires at least one minimum.

Example policy:

```text
100000 x/ruji
50000 rune
```

With enforcement enabled, a new or replaced trigger passes when its attached funds meet at least one configured denom minimum. Exact minimums and higher user-nominated rebates pass. Unsupported denoms alone, missing funds, and amounts below every accepted minimum fail. Extra coins are allowed and all attached coins remain stored for keeper payout.

Policy changes govern only new and replaced triggers. Existing triggers remain executable and keep their stored payout. Raising a minimum therefore does not invalidate old triggers.

With enforcement disabled, trigger creation preserves legacy behavior: arbitrary funds or no funds are accepted.

## Schedule escrow

`Schedule.execution_rebate` remains caller-nominated. When a strategy creates its next schedule trigger, it aggregates duplicate denoms before checking each total against its balance. Sufficient balances send the exact nominated totals to the scheduler. Any insufficient balance returns a condition error and creates no partially funded trigger.

Users should deposit enough funds for expected executions, commonly:

```text
execution count x per-execution rebate
```

## Trigger messages

### `Create(CreateTriggerMsg)`

Creates or replaces a block-height or timestamp trigger. Trigger ID derives from owner, condition, payload, and target contract. Replacement requires the same owner. Any previous rebate is refunded after the new attached rebate passes current policy.

### `Execute(Vec<Uint64>)`

Executes satisfied triggers and sends all stored rebate coins to the transaction sender.

### `ExecuteWithRebateReceiver { ids, rebate_receiver }`

Executes satisfied triggers and sends all stored rebate coins to the validated nominated receiver. This additive message preserves the original `Execute` wire format for existing keepers. Optional executor restrictions always apply to the transaction sender, not the rebate receiver. Successful selection deletes the trigger and calls its target. Downstream target errors are captured by reply handling.

## Trigger queries

- `Filtered { filter, limit }` returns block-height or timestamp triggers matching a range.
- `CanExecute(Uint64)` returns whether one stored trigger's condition is currently satisfied.

## Rollout

1. Migrate scheduler with owner and accepted minimums while leaving enforcement disabled.
2. Have clients query `Config {}` and display accepted denoms and minimum amounts.
3. Let users select a rebate at or above one minimum, fund strategy, and update `Schedule.execution_rebate`.
4. Notify and monitor important users.
5. Owner enables enforcement through `UpdateConfig`.

Existing triggers remain valid. Only new or replacement triggers must satisfy enabled policy.
