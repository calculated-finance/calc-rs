use calc_rs::{
    conditions::Condition,
    core::{Callback, Contract, ContractError, ContractResult},
    exchanger::{ExpectedReceiveAmount, Route},
    scheduler::{CreateTrigger, SchedulerExecuteMsg, TriggerConditionsThreshold},
    thorchain::{MsgDeposit, SwapQuote, SwapQuoteRequest},
};
use cosmwasm_schema::cw_serde;
use cosmwasm_std::{
    to_json_binary, Addr, Coin, Deps, Env, MessageInfo, Response, StdError, StdResult, Uint128,
};

use crate::types::{Exchange, ExchangeConfig};

#[cw_serde]
pub struct ThorchainExchange {
    scheduler_address: Addr,
    affiliate_code: Option<String>,
    affiliate_bps: Option<u64>,
}

impl ThorchainExchange {
    pub fn new(config: ExchangeConfig) -> Self {
        ThorchainExchange {
            scheduler_address: config.scheduler_address,
            affiliate_code: config.affiliate_code,
            affiliate_bps: config.affiliate_bps,
        }
    }
}

impl Exchange for ThorchainExchange {
    fn expected_receive_amount(
        &self,
        deps: Deps,
        swap_amount: &Coin,
        target_denom: &str,
        _route: &Option<Route>,
    ) -> StdResult<ExpectedReceiveAmount> {
        let quote_request = SwapQuoteRequest {
            from_asset: swap_amount.denom.clone(),
            to_asset: target_denom.to_string(),
            amount: swap_amount.amount,
            streaming_interval: Uint128::zero(),
            streaming_quantity: Uint128::zero(),
            destination: "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka".to_string(),
            refund_address: "sthor17pfp4qvy5vrmtjar7kntachm0cfm9m9azl3jka".to_string(),
            affiliate: self
                .affiliate_code
                .clone()
                .map_or_else(std::vec::Vec::new, |c| vec![c]),
            affiliate_bps: self
                .affiliate_bps
                .map_or_else(std::vec::Vec::new, |b| vec![b]),
        };

        let quote = SwapQuote::get(deps.querier, &quote_request)
            .map_err(|e| StdError::generic_err(format!("Failed to get swap quote: {e}")))?;

        Ok(ExpectedReceiveAmount {
            receive_amount: Coin::new(quote.expected_amount_out, target_denom),
            slippage_bps: quote.fees.map(|f| f.slippage_bps).unwrap_or(0).into(),
        })
    }

    fn swap(
        &self,
        deps: Deps,
        env: &Env,
        _info: &MessageInfo,
        swap_amount: &Coin,
        minimum_receive_amount: &Coin,
        maximum_slippage_bps: u128,
        _route: &Option<Route>,
        recipient: Addr,
        on_complete: Option<Callback>,
    ) -> ContractResult {
        let quote_request = SwapQuoteRequest {
            from_asset: swap_amount.denom.clone(),
            to_asset: minimum_receive_amount.denom.clone(),
            amount: swap_amount.amount,
            streaming_interval: Uint128::zero(),
            streaming_quantity: Uint128::zero(),
            destination: recipient.to_string(),
            refund_address: recipient.to_string(),
            affiliate: self
                .affiliate_code
                .clone()
                .map_or_else(std::vec::Vec::new, |c| vec![c]),
            affiliate_bps: self
                .affiliate_bps
                .map_or_else(std::vec::Vec::new, |b| vec![b]),
        };

        let quote = SwapQuote::get(deps.querier, &quote_request)
            .map_err(|e| StdError::generic_err(format!("Failed to get swap quote: {e}")))?;

        if quote.expected_amount_out < minimum_receive_amount.amount {
            return Err(ContractError::generic_err(format!(
                "Expected amount out {} is less than minimum receive amount {}",
                quote.expected_amount_out, minimum_receive_amount.amount
            )));
        }

        if let Some(fees) = quote.fees {
            if fees.slippage_bps as u128 > maximum_slippage_bps {
                return Err(ContractError::generic_err(format!(
                    "Slippage too high: {} > {}",
                    fees.slippage_bps, maximum_slippage_bps
                )));
            }
        }

        let swap_msg = MsgDeposit {
            memo: quote.memo,
            coins: vec![swap_amount.clone()],
            signer: deps.api.addr_canonicalize(env.contract.address.as_str())?,
        }
        .into_cosmos_msg()?;

        let mut messages = vec![swap_msg];

        if let Some(on_complete) = on_complete {
            // schedule a trigger to execute after the swap is complete, in this instance
            // 1 block later given deposit msgs are processed in the subsequent block and
            // we don't support streaming swaps.
            let after_swap_msg = Contract(self.scheduler_address.clone()).call(
                to_json_binary(&SchedulerExecuteMsg::CreateTrigger(CreateTrigger {
                    conditions: vec![Condition::BlocksCompleted(env.block.height + 1)],
                    threshold: TriggerConditionsThreshold::All,
                    to: on_complete.contract,
                    msg: on_complete.msg,
                }))?,
                on_complete.execution_rebate,
            );

            messages.push(after_swap_msg);
        }

        Ok(Response::default().add_messages(messages))
    }
}

#[cfg(test)]
mod expected_receive_amount_tests {
    use super::*;

    use calc_rs_test::test::mock_dependencies_with_custom_grpc_querier;
    use cosmwasm_std::{ContractResult, SystemResult};
    use prost::Message;
    use rujira_rs::proto::types::{QueryQuoteSwapResponse, QuoteFees};

    #[test]
    fn maps_expected_receive_amount_and_slippage() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();

        let expected_receive_amount = Uint128::new(237463);
        let expected_slippage_bps = 123i64;

        deps.querier.with_grpc_handler(move |_| {
            let mut buf = Vec::new();
            QueryQuoteSwapResponse {
                inbound_address: "0".to_string(),
                inbound_confirmation_blocks: 0,
                inbound_confirmation_seconds: 0,
                outbound_delay_blocks: 0,
                outbound_delay_seconds: 0,
                fees: Some(QuoteFees {
                    asset: "0".to_string(),
                    affiliate: "0".to_string(),
                    outbound: "0".to_string(),
                    liquidity: "0".to_string(),
                    total: "0".to_string(),
                    slippage_bps: expected_slippage_bps,
                    total_bps: 145,
                }),
                router: "0".to_string(),
                expiry: 0,
                warning: "0".to_string(),
                notes: "0".to_string(),
                dust_threshold: "0".to_string(),
                recommended_min_amount_in: "0".to_string(),
                recommended_gas_rate: "0".to_string(),
                gas_rate_units: "0".to_string(),
                memo: "0".to_string(),
                expected_amount_out: expected_receive_amount.clone().to_string(),
                max_streaming_quantity: 1,
                streaming_swap_blocks: 1,
                streaming_swap_seconds: 1,
                total_swap_seconds: 1,
            }
            .encode(&mut buf)
            .unwrap();
            SystemResult::Ok(ContractResult::Ok(buf.into()))
        });

        let swap_amount = Coin::new(100u128, "arb-eth");
        let target_denom = "eth-usdc";

        assert_eq!(
            ThorchainExchange::new(ExchangeConfig {
                scheduler_address: Addr::unchecked("scheduler"),
                affiliate_code: None,
                affiliate_bps: None
            })
            .expected_receive_amount(deps.as_ref(), &swap_amount, target_denom, &None)
            .unwrap(),
            ExpectedReceiveAmount {
                receive_amount: Coin::new(expected_receive_amount, target_denom),
                slippage_bps: expected_slippage_bps as u128
            }
        );
    }
}

#[cfg(test)]
mod swap_tests {
    use super::*;

    use calc_rs::core::ContractError;
    use calc_rs_test::test::mock_dependencies_with_custom_grpc_querier;
    use cosmwasm_std::{
        testing::{message_info, mock_env},
        Addr, Api, Binary, Coin, ContractResult, SubMsg, SystemResult, Uint128,
    };
    use prost::Message;
    use rujira_rs::proto::types::{QueryQuoteSwapRequest, QueryQuoteSwapResponse, QuoteFees};

    #[test]
    fn fails_if_expected_receive_amount_too_low() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();

        let expected_receive_amount = Uint128::new(237463);
        let expected_slippage_bps = 123i64;

        deps.querier.with_grpc_handler(move |_| {
            let mut buf = Vec::new();
            QueryQuoteSwapResponse {
                inbound_address: "0".to_string(),
                inbound_confirmation_blocks: 0,
                inbound_confirmation_seconds: 0,
                outbound_delay_blocks: 0,
                outbound_delay_seconds: 0,
                fees: Some(QuoteFees {
                    asset: "0".to_string(),
                    affiliate: "0".to_string(),
                    outbound: "0".to_string(),
                    liquidity: "0".to_string(),
                    total: "0".to_string(),
                    slippage_bps: expected_slippage_bps,
                    total_bps: 145,
                }),
                router: "0".to_string(),
                expiry: 0,
                warning: "0".to_string(),
                notes: "0".to_string(),
                dust_threshold: "0".to_string(),
                recommended_min_amount_in: "0".to_string(),
                recommended_gas_rate: "0".to_string(),
                gas_rate_units: "0".to_string(),
                memo: "0".to_string(),
                expected_amount_out: expected_receive_amount.clone().to_string(),
                max_streaming_quantity: 1,
                streaming_swap_blocks: 1,
                streaming_swap_seconds: 1,
                total_swap_seconds: 1,
            }
            .encode(&mut buf)
            .unwrap();
            SystemResult::Ok(ContractResult::Ok(buf.into()))
        });

        let swap_amount = Coin::new(100u128, "arb-eth");
        let minimum_receive_amount = Coin::new(expected_receive_amount + Uint128::one(), "eth-eth");

        assert_eq!(
            ThorchainExchange::new(ExchangeConfig {
                scheduler_address: Addr::unchecked("scheduler"),
                affiliate_code: None,
                affiliate_bps: None
            })
            .swap(
                deps.as_ref(),
                &mock_env(),
                &message_info(&Addr::unchecked("sender"), &[swap_amount.clone()]),
                &swap_amount,
                &minimum_receive_amount,
                expected_slippage_bps as u128,
                &None,
                Addr::unchecked("recipient"),
                None
            )
            .unwrap_err(),
            ContractError::generic_err(format!(
                "Expected amount out {} is less than minimum receive amount {}",
                expected_receive_amount, minimum_receive_amount.amount
            ))
        );
    }

    #[test]
    fn fails_if_slippage_bps_too_high() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();

        let expected_receive_amount = Uint128::new(237463);
        let expected_slippage_bps = 123i64;

        deps.querier.with_grpc_handler(move |_| {
            let mut buf = Vec::new();
            QueryQuoteSwapResponse {
                inbound_address: "0".to_string(),
                inbound_confirmation_blocks: 0,
                inbound_confirmation_seconds: 0,
                outbound_delay_blocks: 0,
                outbound_delay_seconds: 0,
                fees: Some(QuoteFees {
                    asset: "0".to_string(),
                    affiliate: "0".to_string(),
                    outbound: "0".to_string(),
                    liquidity: "0".to_string(),
                    total: "0".to_string(),
                    slippage_bps: expected_slippage_bps,
                    total_bps: 145,
                }),
                router: "0".to_string(),
                expiry: 0,
                warning: "0".to_string(),
                notes: "0".to_string(),
                dust_threshold: "0".to_string(),
                recommended_min_amount_in: "0".to_string(),
                recommended_gas_rate: "0".to_string(),
                gas_rate_units: "0".to_string(),
                memo: "0".to_string(),
                expected_amount_out: expected_receive_amount.clone().to_string(),
                max_streaming_quantity: 1,
                streaming_swap_blocks: 1,
                streaming_swap_seconds: 1,
                total_swap_seconds: 1,
            }
            .encode(&mut buf)
            .unwrap();
            SystemResult::Ok(ContractResult::Ok(buf.into()))
        });

        let swap_amount = Coin::new(100u128, "arb-eth");
        let minimum_receive_amount = Coin::new(expected_receive_amount, "eth-eth");

        assert_eq!(
            ThorchainExchange::new(ExchangeConfig {
                scheduler_address: Addr::unchecked("scheduler"),
                affiliate_code: None,
                affiliate_bps: None
            })
            .swap(
                deps.as_ref(),
                &mock_env(),
                &message_info(&Addr::unchecked("sender"), &[swap_amount.clone()]),
                &swap_amount,
                &minimum_receive_amount,
                expected_slippage_bps as u128 - 1,
                &None,
                Addr::unchecked("recipient"),
                None
            )
            .unwrap_err(),
            ContractError::generic_err(format!(
                "Slippage too high: {} > {}",
                expected_slippage_bps,
                expected_slippage_bps as u128 - 1
            ))
        );
    }

    #[test]
    fn executes_swap_if_liquidity_ok() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();

        let expected_receive_amount = Uint128::new(237463);
        let expected_slippage_bps = 123i64;

        deps.querier.with_grpc_handler(move |_| {
            let mut buf = Vec::new();
            QueryQuoteSwapResponse {
                inbound_address: "0".to_string(),
                inbound_confirmation_blocks: 0,
                inbound_confirmation_seconds: 0,
                outbound_delay_blocks: 0,
                outbound_delay_seconds: 0,
                fees: Some(QuoteFees {
                    asset: "0".to_string(),
                    affiliate: "0".to_string(),
                    outbound: "0".to_string(),
                    liquidity: "0".to_string(),
                    total: "0".to_string(),
                    slippage_bps: expected_slippage_bps,
                    total_bps: 145,
                }),
                router: "0".to_string(),
                expiry: 0,
                warning: "0".to_string(),
                notes: "0".to_string(),
                dust_threshold: "0".to_string(),
                recommended_min_amount_in: "0".to_string(),
                recommended_gas_rate: "0".to_string(),
                gas_rate_units: "0".to_string(),
                memo: "=:rune:my-address:237463".to_string(),
                expected_amount_out: expected_receive_amount.clone().to_string(),
                max_streaming_quantity: 1,
                streaming_swap_blocks: 1,
                streaming_swap_seconds: 1,
                total_swap_seconds: 1,
            }
            .encode(&mut buf)
            .unwrap();
            SystemResult::Ok(ContractResult::Ok(buf.into()))
        });

        let swap_amount = Coin::new(100u128, "arb-eth");
        let minimum_receive_amount = Coin::new(expected_receive_amount, "eth-eth");
        let env = mock_env();

        assert_eq!(
            ThorchainExchange::new(ExchangeConfig {
                scheduler_address: Addr::unchecked("scheduler"),
                affiliate_code: None,
                affiliate_bps: None
            })
            .swap(
                deps.as_ref(),
                &env,
                &message_info(&deps.api.addr_make("sender"), &[swap_amount.clone()]),
                &swap_amount,
                &minimum_receive_amount,
                expected_slippage_bps as u128 + 1,
                &None,
                Addr::unchecked("recipient"),
                None
            )
            .unwrap()
            .messages[0],
            SubMsg::new(
                MsgDeposit {
                    memo: "=:rune:my-address:237463".to_string(),
                    coins: vec![swap_amount],
                    signer: deps
                        .api
                        .addr_canonicalize(env.contract.address.as_ref())
                        .unwrap()
                }
                .into_cosmos_msg()
                .unwrap()
            )
        );
    }

    #[test]
    fn schedules_after_complete_if_provided() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();

        let expected_receive_amount = Uint128::new(237463);
        let expected_slippage_bps = 123i64;

        deps.querier.with_grpc_handler(move |_| {
            let mut buf = Vec::new();
            QueryQuoteSwapResponse {
                inbound_address: "0".to_string(),
                inbound_confirmation_blocks: 0,
                inbound_confirmation_seconds: 0,
                outbound_delay_blocks: 0,
                outbound_delay_seconds: 0,
                fees: Some(QuoteFees {
                    asset: "0".to_string(),
                    affiliate: "0".to_string(),
                    outbound: "0".to_string(),
                    liquidity: "0".to_string(),
                    total: "0".to_string(),
                    slippage_bps: expected_slippage_bps,
                    total_bps: 145,
                }),
                router: "0".to_string(),
                expiry: 0,
                warning: "0".to_string(),
                notes: "0".to_string(),
                dust_threshold: "0".to_string(),
                recommended_min_amount_in: "0".to_string(),
                recommended_gas_rate: "0".to_string(),
                gas_rate_units: "0".to_string(),
                memo: "=:rune:my-address:237463".to_string(),
                expected_amount_out: expected_receive_amount.clone().to_string(),
                max_streaming_quantity: 1,
                streaming_swap_blocks: 1,
                streaming_swap_seconds: 1,
                total_swap_seconds: 1,
            }
            .encode(&mut buf)
            .unwrap();
            SystemResult::Ok(ContractResult::Ok(buf.into()))
        });

        let swap_amount = Coin::new(100u128, "arb-eth");
        let minimum_receive_amount = Coin::new(expected_receive_amount, "eth-eth");
        let env = mock_env();

        let config = ExchangeConfig {
            scheduler_address: Addr::unchecked("scheduler"),
            affiliate_code: None,
            affiliate_bps: None,
        };

        assert_eq!(
            ThorchainExchange::new(config.clone())
                .swap(
                    deps.as_ref(),
                    &env,
                    &message_info(&deps.api.addr_make("sender"), &[swap_amount.clone()]),
                    &swap_amount,
                    &minimum_receive_amount,
                    expected_slippage_bps as u128 + 1,
                    &None,
                    Addr::unchecked("recipient"),
                    Some(Callback {
                        contract: Addr::unchecked("twap"),
                        msg: to_json_binary("dummy message").unwrap(),
                        execution_rebate: vec![Coin::new(1u128, "rune")],
                    })
                )
                .unwrap()
                .messages[1],
            SubMsg::new(
                Contract(config.scheduler_address.clone()).call(
                    to_json_binary(&SchedulerExecuteMsg::CreateTrigger(CreateTrigger {
                        conditions: vec![Condition::BlocksCompleted(env.block.height + 1)],
                        threshold: TriggerConditionsThreshold::All,
                        to: Addr::unchecked("twap"),
                        msg: to_json_binary("dummy message").unwrap(),
                    }))
                    .unwrap(),
                    vec![Coin::new(1u128, "rune")],
                )
            ),
        );
    }

    #[test]
    fn includes_affiliate_if_configured() {
        let mut deps = mock_dependencies_with_custom_grpc_querier();

        let from_asset = "arb-eth".to_string();
        let swap_amount = Coin::new(100u128, from_asset.clone());
        let to_asset = "eth-eth".to_string();
        let minimum_receive_amount = Coin::new(128u128, to_asset.clone());

        let affiliate_code = Some("rj".to_string());
        let affiliate_bps = Some(10);

        let config = ExchangeConfig {
            scheduler_address: Addr::unchecked("scheduler"),
            affiliate_code: affiliate_code.clone(),
            affiliate_bps,
        };

        deps.querier.with_grpc_handler(move |query| {
            assert_eq!(query.path, "/types.Query/QuoteSwap");

            let request = QueryQuoteSwapRequest {
                from_asset: from_asset.clone(),
                to_asset: to_asset.clone(),
                amount: swap_amount.amount.to_string(),
                streaming_interval: "0".to_string(),
                streaming_quantity: "0".to_string(),
                destination: "recipient".to_string(),
                refund_address: "recipient".to_string(),
                affiliate: vec![affiliate_code.clone().unwrap()],
                affiliate_bps: vec![affiliate_bps.unwrap().to_string()],
                height: "".to_string(),
                tolerance_bps: "".to_string(),
                liquidity_tolerance_bps: "".to_string(),
            };

            let mut buf = Vec::new();
            request.encode(&mut buf).unwrap();
            assert_eq!(query.data, Binary::from(buf));

            SystemResult::Ok(ContractResult::Ok(to_json_binary(&"ignored").unwrap()))
        });

        ThorchainExchange::new(config.clone())
            .swap(
                deps.as_ref(),
                &mock_env(),
                &message_info(&deps.api.addr_make("sender"), &[swap_amount.clone()]),
                &swap_amount,
                &minimum_receive_amount,
                100,
                &None,
                Addr::unchecked("recipient"),
                Some(Callback {
                    contract: Addr::unchecked("twap"),
                    msg: to_json_binary("dummy message").unwrap(),
                    execution_rebate: vec![Coin::new(1u128, "rune")],
                }),
            )
            .unwrap_err();
    }
}
