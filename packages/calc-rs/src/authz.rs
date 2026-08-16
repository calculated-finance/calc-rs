use cosmwasm_std::{Addr, AnyMsg, Binary, Coin, CosmosMsg};
use prost::Message;

pub const MSG_EXEC_TYPE_URL: &str = "/cosmos.authz.v1beta1.MsgExec";
pub const MSG_SEND_TYPE_URL: &str = "/cosmos.bank.v1beta1.MsgSend";
pub const MSG_EXECUTE_CONTRACT_TYPE_URL: &str = "/cosmwasm.wasm.v1.MsgExecuteContract";

#[derive(Clone, PartialEq, Message)]
pub struct ProtoAny {
    #[prost(string, tag = "1")]
    pub type_url: String,
    #[prost(bytes = "vec", tag = "2")]
    pub value: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
pub struct ProtoCoin {
    #[prost(string, tag = "1")]
    pub denom: String,
    #[prost(string, tag = "2")]
    pub amount: String,
}

#[derive(Clone, PartialEq, Message)]
pub struct MsgSend {
    #[prost(string, tag = "1")]
    pub from_address: String,
    #[prost(string, tag = "2")]
    pub to_address: String,
    #[prost(message, repeated, tag = "3")]
    pub amount: Vec<ProtoCoin>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MsgExecuteContract {
    #[prost(string, tag = "1")]
    pub sender: String,
    #[prost(string, tag = "2")]
    pub contract: String,
    #[prost(bytes = "vec", tag = "3")]
    pub msg: Vec<u8>,
    #[prost(message, repeated, tag = "4")]
    pub funds: Vec<ProtoCoin>,
}

#[derive(Clone, PartialEq, Message)]
pub struct MsgExec {
    #[prost(string, tag = "1")]
    pub grantee: String,
    #[prost(message, repeated, tag = "2")]
    pub msgs: Vec<ProtoAny>,
}

impl From<Coin> for ProtoCoin {
    fn from(value: Coin) -> Self {
        Self {
            denom: value.denom,
            amount: value.amount.to_string(),
        }
    }
}

impl From<AnyMsg> for ProtoAny {
    fn from(value: AnyMsg) -> Self {
        Self {
            type_url: value.type_url,
            value: value.value.to_vec(),
        }
    }
}

pub fn bank_send(from: &Addr, to: &Addr, amount: Vec<Coin>) -> AnyMsg {
    AnyMsg {
        type_url: MSG_SEND_TYPE_URL.to_string(),
        value: MsgSend {
            from_address: from.to_string(),
            to_address: to.to_string(),
            amount: amount.into_iter().map(ProtoCoin::from).collect(),
        }
        .encode_to_vec()
        .into(),
    }
}

pub fn execute_contract(sender: &Addr, contract: &Addr, msg: Binary, funds: Vec<Coin>) -> AnyMsg {
    AnyMsg {
        type_url: MSG_EXECUTE_CONTRACT_TYPE_URL.to_string(),
        value: MsgExecuteContract {
            sender: sender.to_string(),
            contract: contract.to_string(),
            msg: msg.to_vec(),
            funds: funds.into_iter().map(ProtoCoin::from).collect(),
        }
        .encode_to_vec()
        .into(),
    }
}

pub fn exec(grantee: &Addr, msgs: Vec<AnyMsg>) -> CosmosMsg {
    CosmosMsg::Any(AnyMsg {
        type_url: MSG_EXEC_TYPE_URL.to_string(),
        value: MsgExec {
            grantee: grantee.to_string(),
            msgs: msgs.into_iter().map(ProtoAny::from).collect(),
        }
        .encode_to_vec()
        .into(),
    })
}

#[cfg(test)]
mod tests {
    use cosmwasm_std::{coin, Addr, CosmosMsg};

    use super::*;

    #[test]
    fn encodes_authz_exec_with_bank_and_wasm_messages() {
        let grantee = Addr::unchecked("strategy");
        let granter = Addr::unchecked("wallet");
        let collector = Addr::unchecked("collector");
        let pair = Addr::unchecked("pair");

        let msg = exec(
            &grantee,
            vec![
                bank_send(&granter, &collector, vec![coin(2, "rune")]),
                execute_contract(
                    &granter,
                    &pair,
                    Binary::from(br#"{"swap":{}}"#.as_slice()),
                    vec![coin(998, "rune")],
                ),
            ],
        );

        let CosmosMsg::Any(msg) = msg else {
            panic!("expected Any message");
        };
        assert_eq!(msg.type_url, MSG_EXEC_TYPE_URL);

        let exec = MsgExec::decode(msg.value.as_slice()).unwrap();
        assert_eq!(exec.grantee, grantee.as_str());
        assert_eq!(exec.msgs.len(), 2);
        assert_eq!(exec.msgs[0].type_url, MSG_SEND_TYPE_URL);
        assert_eq!(exec.msgs[1].type_url, MSG_EXECUTE_CONTRACT_TYPE_URL);

        let send = MsgSend::decode(exec.msgs[0].value.as_slice()).unwrap();
        assert_eq!(send.from_address, granter.as_str());
        assert_eq!(send.to_address, collector.as_str());
        assert_eq!(send.amount[0].amount, "2");

        let wasm = MsgExecuteContract::decode(exec.msgs[1].value.as_slice()).unwrap();
        assert_eq!(wasm.sender, granter.as_str());
        assert_eq!(wasm.contract, pair.as_str());
        assert_eq!(wasm.funds[0].amount, "998");
    }
}
