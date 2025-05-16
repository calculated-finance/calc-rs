use std::any::type_name;

use cosmwasm_std::{Order, StdError, StdResult, Storage};
use cw_storage_plus::{Bound, Item, Map};

use crate::types::{Pair, SwapCache};

pub const SWAP_CACHE: Item<SwapCache> = Item::new("swap_cache_v1");

const PAIRS: Map<String, Pair> = Map::new("pairs_v1");

pub fn save_pair(storage: &mut dyn Storage, pair: &Pair) -> StdResult<()> {
    PAIRS.save(storage, key_from(pair.denoms()), pair)
}

fn key_from(mut denoms: [String; 2]) -> String {
    denoms.sort();
    format!("{}-{}", denoms[0], denoms[1])
}

pub fn find_pair(storage: &dyn Storage, denoms: [String; 2]) -> StdResult<Pair> {
    PAIRS
        .load(storage, key_from(denoms))
        .map_err(|_| StdError::not_found(type_name::<Pair>()))
}

pub fn get_pairs(
    storage: &dyn Storage,
    start_after: Option<[String; 2]>,
    limit: Option<u16>,
) -> Vec<Pair> {
    PAIRS
        .range(
            storage,
            start_after.map(|denoms| Bound::exclusive(key_from(denoms))),
            None,
            Order::Ascending,
        )
        .take(limit.unwrap_or(30) as usize)
        .flat_map(|result| result.map(|(_, pair)| pair))
        .collect::<Vec<Pair>>()
}

pub fn delete_pair(storage: &mut dyn Storage, pair: &Pair) {
    PAIRS.remove(storage, key_from(pair.denoms()))
}

#[cfg(test)]
mod find_pair_tests {
    use super::*;
    use cosmwasm_std::{testing::mock_dependencies, Addr};

    impl Default for Pair {
        fn default() -> Self {
            Pair {
                base_denom: "uruji".to_string(),
                quote_denom: "usdc".to_string(),
                address: Addr::unchecked("pair-address"),
                decimal_delta: 0,
                price_precision: 3,
            }
        }
    }

    #[test]
    fn saves_and_finds_pair() {
        let mut deps = mock_dependencies();
        let pair = Pair::default();

        save_pair(deps.as_mut().storage, &pair).unwrap();

        let saved_pair = find_pair(&deps.storage, pair.denoms()).unwrap();
        assert_eq!(pair, saved_pair);
    }

    #[test]
    fn saves_and_finds_pair_with_denoms_reversed() {
        let mut deps = mock_dependencies();
        let pair = Pair::default();

        save_pair(deps.as_mut().storage, &pair).unwrap();

        let denoms = [pair.denoms()[1].clone(), pair.denoms()[0].clone()];

        let saved_pair = find_pair(&deps.storage, denoms).unwrap();
        assert_eq!(pair, saved_pair);
    }

    #[test]
    fn find_pair_that_does_not_exist_fails() {
        let deps = mock_dependencies();

        let result = find_pair(&deps.storage, Pair::default().denoms()).unwrap_err();

        assert_eq!(result.to_string(), "fin::types::pair::Pair not found");
    }
}

#[cfg(test)]
mod get_pairs_tests {
    use cosmwasm_std::{testing::mock_dependencies, Addr};

    use crate::types::Pair;

    use super::{get_pairs, save_pair};

    #[test]
    fn fetches_all_pairs() {
        let mut deps = mock_dependencies();

        for i in 0..10 {
            let pair = Pair {
                base_denom: format!("base_denom_{}", i),
                quote_denom: format!("quote_denom_{}", i),
                address: Addr::unchecked(format!("address_{}", i)),
                decimal_delta: 0,
                price_precision: 3,
            };

            save_pair(deps.as_mut().storage, &pair).unwrap();
        }

        let pairs = get_pairs(deps.as_ref().storage, None, None);

        assert_eq!(pairs.len(), 10);
    }

    #[test]
    fn fetches_all_pairs_with_limit() {
        let mut deps = mock_dependencies();

        for i in 0..10 {
            let pair = Pair {
                base_denom: format!("base_denom_{}", i),
                quote_denom: format!("quote_denom_{}", i),
                address: Addr::unchecked(format!("address_{}", i)),
                decimal_delta: 0,
                price_precision: 3,
            };

            save_pair(deps.as_mut().storage, &pair).unwrap();
        }

        let pairs = get_pairs(deps.as_ref().storage, None, Some(5));

        assert_eq!(pairs.len(), 5);
    }

    #[test]
    fn fetches_all_pairs_with_start_after() {
        let mut deps = mock_dependencies();

        for i in 0..10 {
            let pair = Pair {
                base_denom: format!("base_denom_{}", i),
                quote_denom: format!("quote_denom_{}", i),
                address: Addr::unchecked(format!("address_{}", i)),
                decimal_delta: 0,
                price_precision: 3,
            };

            save_pair(deps.as_mut().storage, &pair).unwrap();
        }

        let pairs = get_pairs(
            deps.as_ref().storage,
            Some(["base_denom_5".to_string(), "quote_denom_5".to_string()]),
            None,
        );

        assert_eq!(pairs.len(), 4);
        assert_eq!(pairs[0].base_denom, "base_denom_6");
    }

    #[test]
    fn fetches_all_pairs_with_start_after_and_limit() {
        let mut deps = mock_dependencies();

        for i in 0..10 {
            let pair = Pair {
                base_denom: format!("base_denom_{}", i),
                quote_denom: format!("quote_denom_{}", i),
                address: Addr::unchecked(format!("address_{}", i)),
                decimal_delta: 0,
                price_precision: 3,
            };

            save_pair(deps.as_mut().storage, &pair).unwrap();
        }

        let pairs = get_pairs(
            deps.as_ref().storage,
            Some(["base_denom_3".to_string(), "quote_denom_3".to_string()]),
            Some(2),
        );

        assert_eq!(pairs.len(), 2);
        assert_eq!(pairs[0].base_denom, "base_denom_4");
    }
}
