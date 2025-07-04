use std::{collections::HashMap, u8};

use cosmwasm_schema::cw_serde;
use cosmwasm_std::{Coin, Coins, StdResult};

use crate::actions::distribute::Recipient;

#[cw_serde]
#[derive(Default)]
pub struct Statistics {
    pub swapped: Vec<Coin>,
    pub filled: Vec<Coin>,
    pub distributed: Vec<(Recipient, Vec<Coin>)>,
    pub withdrawn: Vec<Coin>,
}


impl Statistics {
    pub fn add(self, other: Statistics) -> StdResult<Statistics> {
        let mut swapped = Coins::try_from(self.swapped.clone()).unwrap_or(Coins::default());
        let mut filled = Coins::try_from(self.filled.clone()).unwrap_or(Coins::default());
        let mut withdrawn = Coins::try_from(self.withdrawn.clone()).unwrap_or(Coins::default());

        for coin in other.swapped {
            swapped.add(coin)?;
        }

        for coin in other.filled {
            filled.add(coin)?;
        }

        for coin in other.withdrawn {
            withdrawn.add(coin)?;
        }

        let mut recipients_map: HashMap<String, Recipient> = HashMap::new();
        let mut distributed_map: HashMap<String, Coins> = HashMap::new();

        for (recipient, amounts) in self
            .distributed
            .iter()
            .chain(other.distributed.clone().iter())
        {
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
            swapped: swapped.into_vec(),
            filled: filled.into_vec(),
            distributed,
            withdrawn: withdrawn.into_vec(),
        })
    }
}
