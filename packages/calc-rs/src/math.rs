use cosmwasm_std::{CheckedMultiplyRatioError, Decimal, Uint128};

pub fn checked_mul(a: Uint128, b: Decimal) -> Result<Uint128, CheckedMultiplyRatioError> {
    a.checked_multiply_ratio(
        b.atomics(),
        Uint128::new(10).checked_pow(b.decimal_places()).unwrap(),
    )
}
