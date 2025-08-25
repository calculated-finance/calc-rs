use std::hash::{DefaultHasher, Hasher};

use calc_rs::{
    core::{Contract, ContractError, ContractResult},
    manager::{ManagerConfig, ManagerExecuteMsg, ManagerQueryMsg},
    strategy::{StrategyExecuteMsg, StrategyQueryMsg},
    tokenizer::{TokenizerConfig, TokenizerExecuteMsg, TokenizerInstantiateMsg, TokenizerQueryMsg},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    entry_point, instantiate2_address, to_json_binary, BankMsg, Coin, Coins, Decimal, Uint128,
};
use cosmwasm_std::{Binary, Deps, DepsMut, Env, MessageInfo, Reply, Response, StdResult};
use rujira_rs::TokenFactory;

use crate::state::{BASE_DENOM, DENOM, DESCRIPTION, ORACLES, STRATEGY};

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn instantiate(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: TokenizerInstantiateMsg,
) -> ContractResult {
    if !info.funds.is_empty() {
        return Err(ContractError::generic_err(format!(
            "Cannot instantiate tokenized strategy with funds, use the Deposit msg to mint {}",
            msg.token_metadata.symbol
        )));
    }

    DENOM.save(deps.storage, &msg.token_metadata.name)?;
    BASE_DENOM.save(deps.storage, &msg.base_denom)?;
    ORACLES.save(deps.storage, &msg.oracles)?;

    let strategy_id = deps
        .querier
        .query_wasm_smart::<Uint128>(&msg.manager_address, &ManagerQueryMsg::Count {})?;

    let mut hash = DefaultHasher::new();

    hash.write(&env.contract.address.as_bytes());
    hash.write(&strategy_id.to_le_bytes());
    hash.write(&env.block.height.to_le_bytes());

    let salt = hash.finish().to_le_bytes();

    let manager_config = deps
        .querier
        .query_wasm_smart::<ManagerConfig>(&msg.manager_address, &ManagerQueryMsg::Config {})?;

    let strategy_address = deps.api.addr_humanize(
        &instantiate2_address(
            deps.querier
                .query_wasm_code_info(manager_config.strategy_code_id)?
                .checksum
                .as_slice(),
            &deps.api.addr_canonicalize(env.contract.address.as_str())?,
            &salt,
        )
        .map_err(|e| {
            ContractError::generic_err(format!("Failed to instantiate contract address: {e}"))
        })?,
    )?;

    STRATEGY.save(deps.storage, &strategy_address)?;

    let instantiate_strategy_msg = Contract(msg.manager_address).call(
        to_json_binary(&ManagerExecuteMsg::Instantiate {
            source: None,
            owner: None,
            label: msg.label,
            affiliates: msg.affiliates,
            nodes: msg.nodes,
        })?,
        info.funds,
    );

    let token_factory = TokenFactory::new(&env, &msg.token_metadata.name);
    let create_token_msg = token_factory.create_msg(msg.token_metadata);

    Ok(Response::new()
        .add_message(instantiate_strategy_msg)
        .add_message(create_token_msg))
}

#[cw_serde]
pub struct MigrateMsg {}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn migrate(_deps: DepsMut, _env: Env, _msg: MigrateMsg) -> ContractResult {
    Ok(Response::new())
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn execute(
    deps: DepsMut,
    env: Env,
    info: MessageInfo,
    msg: TokenizerExecuteMsg,
) -> ContractResult {
    match msg {
        TokenizerExecuteMsg::Deposit {} => {
            if info.funds.is_empty() {
                return Err(ContractError::generic_err(
                    "Must include funds in a Deposit",
                ));
            }

            let value = get_strategy_value(deps.as_ref())?;

            let deposit_strategy_funds_msg = BankMsg::Send {
                to_address: STRATEGY.load(deps.storage)?.to_string(),
                amount: info.funds,
            };

            let mint_msg = Contract(env.contract.address).call(
                to_json_binary(&TokenizerExecuteMsg::Mint {
                    previous_value: value,
                })?,
                vec![],
            );

            Ok(Response::new()
                .add_message(deposit_strategy_funds_msg)
                .add_message(mint_msg))
        }
        TokenizerExecuteMsg::Withdraw {} => {
            let denom = DENOM.load(deps.storage)?;

            if info.funds.len() != 1 || info.funds[0].denom != denom {
                return Err(ContractError::generic_err(format!(
                    "Must only deposit {denom} when withdrawing funds"
                )));
            }

            let burn_amount = info.funds[0].amount;

            let token_factory = TokenFactory::new(&env, &DENOM.load(deps.storage)?);
            let token_supply = token_factory.supply(deps.querier)?;
            let burn_proportion = Decimal::from_ratio(burn_amount, token_supply);

            let balances = deps.querier.query_wasm_smart::<Vec<Coin>>(
                &STRATEGY.load(deps.storage)?,
                &StrategyQueryMsg::Balances {},
            )?;

            let mut withdrawal = Vec::with_capacity(balances.len());

            for balance in balances {
                withdrawal.push(Coin::new(
                    balance.amount.mul_floor(burn_proportion),
                    balance.denom,
                ));
            }

            let strategy_address = STRATEGY.load(deps.storage)?;

            let cancel_strategy_msg = Contract(strategy_address.clone())
                .call(to_json_binary(&StrategyExecuteMsg::Cancel {})?, vec![]);

            let withdraw_strategy_msg = Contract(strategy_address.clone()).call(
                to_json_binary(&StrategyExecuteMsg::Withdraw(withdrawal.clone()))?,
                vec![],
            );

            let execute_strategy_msg = Contract(strategy_address)
                .call(to_json_binary(&StrategyExecuteMsg::Execute {})?, vec![]);

            let distribute_msg = BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: withdrawal,
            };

            let value = get_strategy_value(deps.as_ref())?;

            let burn_msg = Contract(env.contract.address).call(
                to_json_binary(&TokenizerExecuteMsg::Burn {
                    previous_value: value,
                })?,
                vec![],
            );

            Ok(Response::new()
                .add_message(cancel_strategy_msg)
                .add_message(withdraw_strategy_msg)
                .add_message(execute_strategy_msg)
                .add_message(distribute_msg)
                .add_message(burn_msg))
        }
        TokenizerExecuteMsg::Mint { previous_value } => {
            if info.sender != env.contract.address {
                return Err(ContractError::generic_err(
                    "Mint can only be called by the contract itself",
                ));
            }

            let post_value = get_strategy_value(deps.as_ref())?;
            let value_delta = post_value.amount.checked_sub(previous_value.amount)?;

            let token_factory = TokenFactory::new(&env, &DENOM.load(deps.storage)?);
            let token_supply = token_factory.supply(deps.querier)?;

            let mint_amount = if token_supply.is_zero() || previous_value.amount.is_zero() {
                value_delta
            } else {
                token_supply.mul_floor(Decimal::from_ratio(value_delta, previous_value.amount))
            };

            let mint_msg = token_factory.mint_msg(mint_amount, info.sender);

            Ok(Response::new().add_message(mint_msg))
        }
        TokenizerExecuteMsg::Burn { previous_value } => {
            if info.sender != env.contract.address {
                return Err(ContractError::generic_err(
                    "Burn can only be called by the contract itself",
                ));
            }

            let post_value = get_strategy_value(deps.as_ref())?;
            let value_delta = previous_value.amount.checked_sub(post_value.amount)?;

            let token_factory = TokenFactory::new(&env, &DENOM.load(deps.storage)?);
            let token_supply = token_factory.supply(deps.querier)?;

            let burn_amount = if token_supply.is_zero() || previous_value.amount.is_zero() {
                Uint128::zero()
            } else {
                token_supply.mul_floor(Decimal::from_ratio(value_delta, previous_value.amount))
            };

            let burn_msg = token_factory.burn_msg(burn_amount);

            let withdraw_strategy_funds_msg = BankMsg::Send {
                to_address: info.sender.to_string(),
                amount: vec![Coin::new(
                    value_delta.u128(),
                    BASE_DENOM.load(deps.storage)?,
                )],
            };

            Ok(Response::new()
                .add_message(burn_msg)
                .add_message(withdraw_strategy_funds_msg))
        }
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn query(deps: Deps, _env: Env, msg: TokenizerQueryMsg) -> StdResult<Binary> {
    match msg {
        TokenizerQueryMsg::Config {} => to_json_binary(&TokenizerConfig {
            denom: DENOM.load(deps.storage)?,
            base_denom: BASE_DENOM.load(deps.storage)?,
            oracles: ORACLES.load(deps.storage)?,
            strategy_address: STRATEGY.load(deps.storage)?,
            description: DESCRIPTION.load(deps.storage)?,
        }),
        TokenizerQueryMsg::Value {} => to_json_binary(&get_strategy_value(deps)?),
    }
}

#[cfg_attr(not(feature = "library"), entry_point)]
pub fn reply(_deps: DepsMut, _env: Env, _reply: Reply) -> ContractResult {
    Ok(Response::new())
}

fn get_strategy_value(deps: Deps) -> StdResult<Coin> {
    let oracles = ORACLES.load(deps.storage)?;
    let strategy = STRATEGY.load(deps.storage)?;

    let balances = Coins::try_from(
        deps.querier
            .query_wasm_smart::<Vec<Coin>>(strategy, &StrategyQueryMsg::Balances {})?,
    )?;

    let quote_denom = BASE_DENOM.load(deps.storage)?;

    let mut total_value = Uint128::zero();

    for (base_denom, oracle) in oracles {
        let asset_price = oracle.query_price(deps, &base_denom, &quote_denom)?;
        total_value += balances.amount_of(&base_denom).mul_floor(asset_price);
    }

    Ok(Coin::new(total_value, quote_denom))
}
