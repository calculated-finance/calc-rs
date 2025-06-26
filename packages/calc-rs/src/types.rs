use std::{collections::HashMap, time::Duration, u8};

use anybuf::Anybuf;
use cosmwasm_schema::{cw_serde, QueryResponses};
use cosmwasm_std::{
    to_json_string, Addr, AnyMsg, BankMsg, Binary, BlockInfo, CanonicalAddr, CheckedFromRatioError,
    CheckedMultiplyRatioError, Coin, CoinsError, CosmosMsg, Decimal, Deps, Env, Event,
    Instantiate2AddressError, MessageInfo, OverflowError, Response, StdError, StdResult, Timestamp,
    Uint128, WasmMsg,
};
use cw_storage_plus::{Key, Prefixer, PrimaryKey};
use rujira_rs::{
    fin::{OrderResponse, Price, QueryMsg, Side},
    Layer1Asset, NativeAsset, SecuredAsset,
};
use thiserror::Error;

pub const DEPOSIT_FEE: u128 = 2_000_000; // 0.02 RUNE

#[derive(Error, Debug, PartialEq)]
pub enum ContractError {
    #[error("{0}")]
    Std(#[from] StdError),

    #[error("{0}")]
    Instantiate2Address(#[from] Instantiate2AddressError),

    #[error("{0}")]
    CheckedMultiplyRatioError(#[from] CheckedMultiplyRatioError),

    #[error("{0}")]
    OverflowError(#[from] OverflowError),

    #[error("{0}")]
    CheckedFromRatioError(#[from] CheckedFromRatioError),

    #[error("{0}")]
    CoinsError(#[from] CoinsError),

    #[error("Unauthorized")]
    Unauthorized {},

    #[error("Generic error: {0}")]
    Generic(&'static str),
}

impl ContractError {
    pub fn generic_err(msg: impl Into<String>) -> Self {
        ContractError::Std(StdError::generic_err(msg.into()))
    }
}

pub type ContractResult = Result<Response, ContractError>;

#[cw_serde]
pub struct ManagerConfig {
    pub admin: Addr,
    pub code_id: u64,
    pub fee_collector: Addr,
}

#[cw_serde]
pub struct ExpectedReceiveAmount {
    pub receive_amount: Coin,
    pub slippage: Decimal,
}

#[cw_serde]
pub enum Condition {
    TimestampElapsed(Timestamp),
    BlocksCompleted(u64),
    LimitOrderFilled {
        owner: Addr,
        pair_address: Addr,
        side: Side,
        price: Price,
    },
    PriceThresholdMet {
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    },
    SlippageLimitMet {
        swap_amount: Coin,
        target_denom: String,
        slippage_bps: u64,
    },
    BalanceMet {
        address: Addr,
        balance: Coin,
    },
}

impl Condition {
    pub fn is_satisfied(&self, deps: Deps, env: &Env) -> StdResult<bool> {
        match self {
            Condition::TimestampElapsed(timestamp) => Ok(env.block.time >= *timestamp),
            Condition::BlocksCompleted(height) => Ok(env.block.height >= *height),
            Condition::LimitOrderFilled {
                owner,
                pair_address,
                side,
                price,
            } => deps
                .querier
                .query_wasm_smart::<OrderResponse>(
                    pair_address,
                    &QueryMsg::Order((owner.to_string(), side.clone(), price.clone())),
                )
                .map(|r| r.remaining.is_zero()),
            Condition::PriceThresholdMet {
                swap_amount,
                minimum_receive_amount,
            } => deps
                .querier
                .query_wasm_smart::<ExpectedReceiveAmount>(
                    swap_amount.denom.clone(),
                    &ExchangeQueryMsg::ExpectedReceiveAmount {
                        swap_amount: swap_amount.clone(),
                        target_denom: minimum_receive_amount.denom.clone(),
                    },
                )
                .map(|r| r.receive_amount.amount >= minimum_receive_amount.amount),
            Condition::SlippageLimitMet {
                swap_amount,
                target_denom,
                slippage_bps,
            } => deps
                .querier
                .query_wasm_smart::<ExpectedReceiveAmount>(
                    swap_amount.denom.clone(),
                    &ExchangeQueryMsg::ExpectedReceiveAmount {
                        swap_amount: swap_amount.clone(),
                        target_denom: target_denom.clone(),
                    },
                )
                .map(|r| r.slippage <= Decimal::bps(*slippage_bps)),
            Condition::BalanceMet { address, balance } => deps
                .querier
                .query_balance(address, balance.denom.clone())
                .map(|r| r.amount >= balance.amount),
        }
    }
}

#[cw_serde]
pub enum ConditionFilter {
    Owner {
        address: Addr,
    },
    Timestamp {
        start: Option<Timestamp>,
        end: Option<Timestamp>,
    },
    BlockHeight {
        start: Option<u64>,
        end: Option<u64>,
    },
}

#[cw_serde]
pub struct AccumulateStatistics {
    pub amount_deposited: Coin,
    pub amount_swapped: Coin,
    pub amount_received: Coin,
}

#[cw_serde]
pub struct DistributorStatistics {
    pub amount_distributed: HashMap<String, Vec<Coin>>,
    pub amount_withdrawn: Vec<Coin>,
}

#[cw_serde]
pub struct NewStatistics {
    pub amount: Coin,
}

#[cw_serde]
pub enum StrategyStatistics {
    Accumulate(AccumulateStatistics),
    New(NewStatistics),
}

pub fn layer_1_asset(denom: &NativeAsset) -> StdResult<Layer1Asset> {
    let denom_string = denom.denom_string();

    if denom_string.contains("rune") {
        return Ok(Layer1Asset::new("THOR", "RUNE"));
    }

    let (chain, symbol) = denom_string
        .split_once('-')
        .ok_or_else(|| StdError::generic_err(format!("Invalid layer 1 asset: {}", denom)))?;

    Ok(Layer1Asset::new(
        &chain.to_ascii_uppercase(),
        &symbol.to_ascii_uppercase(),
    ))
}

pub fn secured_asset(asset: &Layer1Asset) -> StdResult<SecuredAsset> {
    match asset.denom_string().to_uppercase().split_once(".") {
        Some((chain, symbol)) => {
            if chain == "THOR" && symbol == "RUNE" {
                return Ok(SecuredAsset::new("THOR", "RUNE"));
            }
            Ok(SecuredAsset::new(chain, symbol))
        }
        None => Err(StdError::generic_err(format!(
            "Invalid layer 1 asset: {}",
            asset.denom_string()
        ))),
    }
}

pub struct MsgDeposit {
    pub memo: String,
    pub coins: Vec<Coin>,
    pub signer: CanonicalAddr,
}

impl From<MsgDeposit> for CosmosMsg {
    fn from(value: MsgDeposit) -> Self {
        let coins: Vec<Anybuf> = value
            .coins
            .iter()
            .map(|c| {
                let asset = layer_1_asset(&NativeAsset::new(&c.denom))
                    .unwrap()
                    .denom_string()
                    .to_ascii_uppercase();
                let (chain, symbol) = asset.split_once('.').unwrap();

                Anybuf::new()
                    .append_message(
                        1,
                        &Anybuf::new()
                            .append_string(1, chain)
                            .append_string(2, symbol)
                            .append_string(3, symbol)
                            .append_bool(4, false)
                            .append_bool(5, false)
                            .append_bool(6, c.denom.to_lowercase() != "rune"),
                    )
                    .append_string(2, c.amount.to_string())
            })
            .collect();

        let value = Anybuf::new()
            .append_repeated_message(1, &coins)
            .append_string(2, value.memo)
            .append_bytes(3, value.signer.to_vec());

        CosmosMsg::Any(AnyMsg {
            type_url: "/types.MsgDeposit".to_string(),
            value: value.as_bytes().into(),
        })
    }
}

#[cw_serde]
pub enum Recipient {
    Bank { address: Addr },
    Wasm { address: Addr, msg: Binary },
    Deposit { memo: String },
}

impl Recipient {
    pub fn key(&self) -> String {
        match self {
            Recipient::Bank { address } | Recipient::Wasm { address, .. } => address.to_string(),
            Recipient::Deposit { memo } => memo.clone(),
        }
    }
}

#[cw_serde]
pub struct Destination {
    pub shares: Uint128,
    pub recipient: Recipient,
    pub label: Option<String>,
}

#[cw_serde]
pub struct Distribution {
    pub destination: Destination,
    pub amount: Vec<Coin>,
}

impl Distribution {
    pub fn get_msg(self, deps: Deps, env: &Env) -> StdResult<CosmosMsg> {
        match self.destination.recipient {
            Recipient::Bank { address, .. } => Ok(BankMsg::Send {
                to_address: address.into(),
                amount: self.amount,
            }
            .into()),
            Recipient::Wasm { address, msg, .. } => Ok(WasmMsg::Execute {
                contract_addr: address.into(),
                msg,
                funds: self.amount,
            }
            .into()),
            Recipient::Deposit { memo } => Ok(MsgDeposit {
                memo: memo,
                coins: self.amount,
                signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
            }
            .into()),
        }
    }
}

#[cw_serde]
pub enum Schedule {
    Blocks {
        interval: u64,
        previous: Option<u64>,
    },
    Time {
        duration: Duration,
        previous: Option<Timestamp>,
    },
}

impl Schedule {
    pub fn is_due(&self, block: &BlockInfo) -> bool {
        match self {
            Schedule::Blocks { interval, previous } => {
                let last_block = previous.unwrap_or(0);
                block.height >= last_block + interval
            }
            Schedule::Time { duration, previous } => {
                let last_time = previous.unwrap_or(Timestamp::from_seconds(0));
                block.time.seconds() >= last_time.seconds() + duration.as_secs()
            }
        }
    }

    pub fn to_conditions(&self, env: &Env) -> Vec<Condition> {
        match self {
            Schedule::Blocks { interval, previous } => {
                let last_block = previous.unwrap_or(env.block.height);
                vec![Condition::BlocksCompleted(last_block + interval)]
            }
            Schedule::Time { duration, previous } => {
                let last_time =
                    previous.unwrap_or(Timestamp::from_seconds(env.block.time.seconds()));
                vec![Condition::TimestampElapsed(Timestamp::from_seconds(
                    last_time.seconds() + duration.as_secs(),
                ))]
            }
        }
    }
}

#[cw_serde]
pub struct AccumulateStrategyConfig {
    pub owner: Addr,
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub schedule: Schedule,
    pub exchange_contract: Addr,
    pub scheduler_contract: Addr,
    pub execution_rebate: Coin,
    pub affiliate_code: Option<String>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
    pub statistics: AccumulateStatistics,
}

#[cw_serde]
pub struct DistributorConfig {
    pub owner: Addr,
    pub denoms: Vec<String>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
    pub conditions: Vec<Condition>,
}

#[derive()]
#[cw_serde]
pub enum StrategyConfig {
    Accumulate(AccumulateStrategyConfig),
    // Custom(DistributeStrategyConfig),
}

// impl From<InstantiateStrategyCommand> for StrategyConfig {
//     fn from(config: InstantiateStrategyCommand) -> Self {
//         match config {
//             InstantiateStrategyCommand::Accumulate {
//                 owner,
//                 swap_amount,
//                 minimum_receive_amount,
//                 schedule,
//                 exchange_contract,
//                 scheduler_contract,
//                 execution_rebate,
//                 mutable_destinations,
//                 immutable_destinations,
//                 affiliate_code,
//             } => StrategyConfig::Accumulate(AccumulateStrategyConfig {
//                 owner,
//                 swap_amount,
//                 minimum_receive_amount,
//                 schedule,
//                 exchange_contract,
//                 scheduler_contract,
//                 execution_rebate,
//                 mutable_destinations,
//                 immutable_destinations,
//                 affiliate_code,
//                 statistics: AccumulateStatistics {
//                     amount_deposited: Coin {
//                         denom: swap_amount.denom.to_string(),
//                         amount: Uint128::zero(),
//                     },
//                     amount_swapped: Coin {
//                         denom: swap_amount.denom.to_string(),
//                         amount: Uint128::zero(),
//                     },
//                     amount_received: Coin {
//                         denom: minimum_receive_amount.denom.to_string(),
//                         amount: Uint128::zero(),
//                     },
//                 },
//             }),
//             InstantiateStrategyCommand::Distribute {} => {
//                 StrategyConfig::Custom(DistributeStrategyConfig {
//                     owner: Addr::unchecked("custom_strategy_owner"),
//                 })
//             }
//         }
//     }
// }

pub trait Owned {
    fn owner(&self) -> Addr;
}

impl Owned for StrategyConfig {
    fn owner(&self) -> Addr {
        match self {
            StrategyConfig::Accumulate(strategy) => strategy.owner.clone(),
            // StrategyConfig::Custom(strategy) => strategy.owner.clone(),
        }
    }
}

#[cw_serde]
pub enum StrategyStatus {
    Active,
    Paused,
    Archived,
}

impl<'a> Prefixer<'a> for StrategyStatus {
    fn prefix(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

impl<'a> PrimaryKey<'a> for StrategyStatus {
    type Prefix = Self;
    type SubPrefix = Self;
    type Suffix = ();
    type SuperSuffix = ();

    fn key(&self) -> Vec<Key> {
        vec![Key::Val8([self.clone() as u8])]
    }
}

#[cw_serde]
pub struct Affiliate {
    pub code: String,
    pub address: Addr,
    pub bps: u64,
}

#[cw_serde]
pub struct Strategy {
    pub owner: Addr,
    pub contract_address: Addr,
    pub created_at: u64,
    pub updated_at: u64,
    pub label: String,
    pub status: StrategyStatus,
    pub affiliates: Vec<Affiliate>,
}

pub enum DomainEvent {
    StrategyInstantiated {
        contract_address: Addr,
        config: StrategyConfig,
    },
    StrategyPaused {
        contract_address: Addr,
        reason: String,
    },
    StrategyArchived {
        contract_address: Addr,
    },
    StrategyResumed {
        contract_address: Addr,
    },
    StrategyUpdated {
        contract_address: Addr,
        old_config: StrategyConfig,
        new_config: StrategyConfig,
    },
    FundsDeposited {
        contract_address: Addr,
        from: Addr,
        funds: Vec<Coin>,
    },
    FundsWithdrawn {
        contract_address: Addr,
        to: Addr,
        funds: Vec<Coin>,
    },
    FundsDistributed {
        contract_address: Addr,
        to: Vec<Distribution>,
    },
    ExecutionSucceeded {
        contract_address: Addr,
        statistics: StrategyStatistics,
    },
    ExecutionFailed {
        contract_address: Addr,
        reason: String,
    },
    ExecutionSkipped {
        contract_address: Addr,
        reason: String,
    },
    SchedulingSucceeded {
        contract_address: Addr,
        conditions: Vec<Condition>,
    },
    SchedulingFailed {
        contract_address: Addr,
        reason: String,
    },
    SchedulingSkipped {
        contract_address: Addr,
        reason: String,
    },
}

impl From<DomainEvent> for Event {
    fn from(event: DomainEvent) -> Self {
        match event {
            DomainEvent::StrategyInstantiated {
                contract_address,
                config,
            } => Event::new("strategy_created")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "config",
                    to_json_string(&config).expect("Failed to serialize config"),
                ),
            DomainEvent::StrategyPaused {
                contract_address,
                reason,
            } => Event::new("strategy_paused")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::StrategyResumed { contract_address } => Event::new("strategy_resumed")
                .add_attribute("contract_address", contract_address.as_str()),
            DomainEvent::StrategyArchived { contract_address } => Event::new("strategy_archived")
                .add_attribute("contract_address", contract_address.as_str()),
            DomainEvent::StrategyUpdated {
                contract_address,
                old_config,
                new_config,
            } => Event::new("strategy_updated")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "old_config",
                    to_json_string(&old_config).expect("Failed to serialize old config"),
                )
                .add_attribute(
                    "new_config",
                    to_json_string(&new_config).expect("Failed to serialize new config"),
                ),
            DomainEvent::FundsDeposited {
                contract_address,
                from,
                funds: amount,
            } => Event::new("funds_deposited")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("from", from.as_str())
                .add_attribute(
                    "amount",
                    to_json_string(&amount).expect("Failed to serialize amount"),
                ),
            DomainEvent::FundsWithdrawn {
                contract_address,
                to,
                funds: amount,
            } => Event::new("funds_withdrawn")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("to", to.as_str())
                .add_attribute(
                    "amount",
                    to_json_string(&amount).expect("Failed to serialize withdrawn amount"),
                ),
            DomainEvent::FundsDistributed {
                contract_address,
                to: distributions,
            } => Event::new("funds_distributed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "distributions",
                    to_json_string(&distributions).expect("Failed to serialize distributions"),
                ),
            DomainEvent::ExecutionSucceeded {
                contract_address,
                statistics,
            } => Event::new("execution_succeeded")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "statistics",
                    to_json_string(&statistics).expect("Failed to serialize statistics"),
                ),
            DomainEvent::ExecutionFailed {
                contract_address,
                reason: error,
            } => Event::new("execution_failed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("error", error),
            DomainEvent::ExecutionSkipped {
                contract_address,
                reason,
            } => Event::new("execution_skipped")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::SchedulingSucceeded {
                contract_address,
                conditions,
            } => Event::new("scheduling_succeeded")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute(
                    "conditions",
                    to_json_string(&conditions).expect("Failed to serialize conditions"),
                ),
            DomainEvent::SchedulingFailed {
                contract_address,
                reason,
            } => Event::new("scheduling_failed")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
            DomainEvent::SchedulingSkipped {
                contract_address,
                reason,
            } => Event::new("scheduling_skipped")
                .add_attribute("contract_address", contract_address.as_str())
                .add_attribute("reason", reason),
        }
    }
}

pub struct Contract(pub Addr);

impl Contract {
    pub fn addr(&self) -> Addr {
        self.0.clone()
    }

    pub fn call(&self, msg: Binary, funds: Vec<Coin>) -> CosmosMsg {
        WasmMsg::Execute {
            contract_addr: self.addr().into(),
            msg,
            funds,
        }
        .into()
    }
}

#[cw_serde]
pub enum TriggerConditionsThreshold {
    Any,
    All,
}

#[cw_serde]
pub struct Trigger {
    pub id: u64,
    pub owner: Addr,
    pub conditions: Vec<Condition>,
    pub threshold: TriggerConditionsThreshold,
    pub msg: Binary,
    pub to: Addr,
    pub execution_rebate: Vec<Coin>,
}

impl Trigger {
    pub fn from_command(info: &MessageInfo, command: CreateTrigger, rebate: Vec<Coin>) -> Self {
        Self {
            id: 0,
            owner: info.sender.clone(),
            conditions: command.conditions,
            threshold: command.threshold,
            msg: command.msg,
            to: command.to,
            execution_rebate: rebate,
        }
    }

    pub fn can_execute(&self, deps: Deps, env: &Env) -> StdResult<bool> {
        Ok(match self.threshold {
            TriggerConditionsThreshold::All => self
                .conditions
                .iter()
                .all(|c| c.is_satisfied(deps, env).unwrap_or(false)),
            TriggerConditionsThreshold::Any => self
                .conditions
                .iter()
                .any(|c| c.is_satisfied(deps, env).unwrap_or(false)),
        })
    }
}

#[cw_serde]
pub struct ManagerInstantiateMsg {
    pub code_id: u64,
    pub fee_collector: Addr,
}

#[cw_serde]
pub struct ManagerMigrateMsg {
    pub code_id: u64,
    pub fee_collector: Addr,
}

#[cw_serde]
pub enum ManagerExecuteMsg {
    InstantiateStrategy {
        owner: Addr,
        label: String,
        strategy: InstantiateStrategyCommand,
    },
    ExecuteStrategy {
        contract_address: Addr,
        msg: Option<Binary>,
    },
    PauseStrategy {
        contract_address: Addr,
    },
    ResumeStrategy {
        contract_address: Addr,
    },
    WithdrawFromStrategy {
        contract_address: Addr,
        amounts: Vec<Coin>,
    },
    UpdateStrategy {
        contract_address: Addr,
        update: StrategyConfig,
    },
    UpdateStatus {
        status: StrategyStatus,
    },
    AddAffiliate {
        affiliate: Affiliate,
    },
    RemoveAffiliate {
        code: String,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum ManagerQueryMsg {
    #[returns(ManagerConfig)]
    Config {},
    #[returns(Strategy)]
    Strategy { address: Addr },
    #[returns(Vec<Strategy>)]
    Strategies {
        owner: Option<Addr>,
        status: Option<StrategyStatus>,
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
    #[returns(Affiliate)]
    Affiliate { code: String },
    #[returns(Vec<Affiliate>)]
    Affiliates {
        start_after: Option<Addr>,
        limit: Option<u16>,
    },
}

#[cw_serde]
pub enum InstantiateStrategyCommand {
    Accumulate {
        owner: Addr,
        swap_amount: Coin,
        minimum_receive_amount: Coin,
        schedule: Schedule,
        exchange_contract: Addr,
        scheduler_contract: Addr,
        execution_rebate: Coin,
        affiliate_code: Option<String>,
        mutable_destinations: Vec<Destination>,
        immutable_destinations: Vec<Destination>,
    },
    Distribute {},
}

#[cw_serde]
pub struct StrategyInstantiateMsg {
    pub fee_collector: Addr,
    pub strategy: InstantiateStrategyCommand,
}

#[cw_serde]
pub enum StrategyExecuteMsg {
    Execute { msg: Option<Binary> },
    Deposit {},
    Withdraw { amounts: Vec<Coin> },
    Pause {},
    Resume {},
    Update { update: StrategyConfig },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum StrategyQueryMsg {
    #[returns(StrategyConfig)]
    Config {},
    #[returns(bool)]
    CanExecute { msg: Option<Binary> },
}

#[cw_serde]
pub struct Callback {
    pub contract: Addr,
    pub msg: Binary,
    pub execution_rebate: Vec<Coin>,
}

#[cw_serde]
pub enum ExchangeExecuteMsg {
    Swap {
        minimum_receive_amount: Coin,
        recipient: Option<Addr>,
        on_complete: Option<Callback>,
    },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum ExchangeQueryMsg {
    #[returns(bool)]
    CanSwap {
        swap_amount: Coin,
        minimum_receive_amount: Coin,
    },
    #[returns(Vec<Coin>)]
    Route {
        swap_amount: Coin,
        target_denom: String,
    },
    #[returns(Decimal)]
    SpotPrice {
        swap_denom: String,
        target_denom: String,
    },
    #[returns(ExpectedReceiveAmount)]
    ExpectedReceiveAmount {
        swap_amount: Coin,
        target_denom: String,
    },
}

#[cw_serde]
pub struct CreateTrigger {
    pub conditions: Vec<Condition>,
    pub threshold: TriggerConditionsThreshold,
    pub to: Addr,
    pub msg: Binary,
}

#[cw_serde]
pub enum SchedulerExecuteMsg {
    CreateTrigger(CreateTrigger),
    SetTriggers(Vec<CreateTrigger>),
    ExecuteTrigger(u64),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum SchedulerQueryMsg {
    #[returns(Vec<Trigger>)]
    Triggers {
        filter: ConditionFilter,
        limit: Option<usize>,
        can_execute: Option<bool>,
    },
    #[returns(bool)]
    CanExecute { id: u64 },
}

#[cw_serde]
pub struct DistributorInstantiateMsg {
    pub owner: Addr,
    pub denoms: Vec<String>,
    pub schedule: Option<Schedule>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
}

pub trait Validate {
    fn validate(&self) -> StdResult<()>;
}

#[cw_serde]
pub enum DistributorExecuteMsg {
    Distribute {},
    Withdraw { amounts: Vec<Coin> },
    Update(DistributorConfig),
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum DistributorQueryMsg {
    #[returns(DistributorConfig)]
    Config,
    #[returns(DistributorStatistics)]
    Statistics,
}

#[cw_serde]
pub struct DcaInstantiateMsg {
    pub owner: Addr,
    pub swap_amount: Coin,
    pub minimum_receive_amount: Coin,
    pub schedule: Schedule,
    pub exchange_contract: Addr,
    pub scheduler_contract: Addr,
    pub execution_rebate: Coin,
    pub affiliate_code: Option<String>,
    pub mutable_destinations: Vec<Destination>,
    pub immutable_destinations: Vec<Destination>,
}

#[cw_serde]
pub enum AccumulatorExecuteMsg {
    Execute {},
    Withdraw { amounts: Vec<Coin> },
}

#[cw_serde]
#[derive(QueryResponses)]
pub enum AccumulatorQueryMsg {
    #[returns(AccumulateStrategyConfig)]
    Config {},
    #[returns(bool)]
    CanExecute {},
}
