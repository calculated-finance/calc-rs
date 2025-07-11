use std::collections::HashMap;

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Coins, StdResult};

use crate::actions::distribution::Recipient;

#[cw_serde]
#[derive(Default)]
pub struct Statistics {
    pub debited: Vec<Coin>,
    pub credited: Vec<(Recipient, Vec<Coin>)>,
}

impl Statistics {
    pub fn update(self, other: Statistics) -> StdResult<Statistics> {
        let mut outgoing = Coins::try_from(self.debited.clone())?;

        for coin in other.debited {
            outgoing.add(coin)?;
        }

        let mut recipients_map: HashMap<String, Recipient> = HashMap::new();
        let mut distributed_map: HashMap<String, Coins> = HashMap::new();

        for (recipient, amounts) in self.credited.iter().chain(other.credited.clone().iter()) {
            recipients_map
                .entry(recipient.key())
                .or_insert_with(|| recipient.clone());

            distributed_map
                .entry(recipient.key())
                .and_modify(|coins| {
                    for amount in amounts {
                        coins.add(amount.clone()).unwrap_or_default();
                    }
                })
                .or_insert(Coins::try_from(amounts.clone())?);
        }

        let mut distributed: Vec<(Recipient, Vec<Coin>)> = Vec::new();

        for (key, coins) in distributed_map.into_iter() {
            let recipient = recipients_map
                .get(&key)
                .expect("Recipient should exist in map");
            distributed.push((recipient.clone(), coins.into_vec()));
        }

        Ok(Statistics {
            debited: outgoing.into_vec(),
            credited: distributed,
        })
    }
}
