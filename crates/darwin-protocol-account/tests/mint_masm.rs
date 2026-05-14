//! End-to-end test of `asm/lib/mint.masm` against the bundled
//! primitives library produced by `build.rs`.

mod common;
use common::run_one;

// ----- par_value ---------------------------------------------------------------

#[test]
fn par_value_no_fee_returns_deposit_value() {
    let out = run_one("darwin::mint", "mint::par_value", vec![100_000, 0]);
    assert_eq!(out, 100_000);
}

#[test]
fn par_value_with_30_bps_fee_keeps_99_70_percent() {
    // 100_000 * 9970 / 10000 = 99_700
    let out = run_one("darwin::mint", "mint::par_value", vec![100_000, 30]);
    assert_eq!(out, 99_700);
}

#[test]
fn par_value_with_full_fee_returns_zero() {
    let out = run_one("darwin::mint", "mint::par_value", vec![100_000, 10_000]);
    assert_eq!(out, 0);
}

#[test]
fn par_value_handles_u64_range_deposit() {
    // 1e15 deposit_value, 30 bps fee
    // result = 1e15 * 9970 / 10000 = 9.97e14
    let out = run_one(
        "darwin::mint",
        "mint::par_value",
        vec![1_000_000_000_000_000, 30],
    );
    assert_eq!(out, 997_000_000_000_000);
}

// ----- standard ----------------------------------------------------------------

#[test]
fn standard_no_fee_par_state_returns_deposit_value() {
    // deposit=100, supply=100, nav=100, fee=0  =>  100
    let out = run_one("darwin::mint", "mint::standard", vec![100, 0, 100, 100]);
    assert_eq!(out, 100);
}

#[test]
fn standard_with_30_bps_fee_at_par() {
    // 100 * 9970 / 10000 = 99 (truncated from 99.7)
    let out = run_one("darwin::mint", "mint::standard", vec![100, 30, 100, 100]);
    assert_eq!(out, 99);
}

#[test]
fn standard_doubles_supply_when_nav_is_half() {
    let out = run_one("darwin::mint", "mint::standard", vec![100, 0, 100, 50]);
    assert_eq!(out, 200);
}

#[test]
fn standard_halves_supply_when_nav_is_double() {
    let out = run_one("darwin::mint", "mint::standard", vec![100, 0, 100, 200]);
    assert_eq!(out, 50);
}

#[test]
fn standard_handles_u64_range_inputs_within_felt_intermediate_budget() {
    // The intermediate product `deposit * 10000 * supply` is computed
    // in felt arithmetic, so the (1.84e19) Goldilocks modulus is the
    // hard ceiling. With deposit=1e8 and supply=1e6, the product
    // * 10000 = 1e18, comfortably under the ceiling.
    //
    // Inputs:
    //   deposit_value = 1e8  ($1 in USD * 1e8)
    //   pre_supply    = 1e6  (1M basket tokens)
    //   pre_nav       = 5e5  (NAV per share = 0.5)
    //   fee           = 0
    // mint = 1e8 * 10000 * 1e6 / (10000 * 5e5) = 2e8.
    let deposit = 100_000_000;
    let fee = 0;
    let supply = 1_000_000;
    let nav = 500_000;
    let out = run_one(
        "darwin::mint",
        "mint::standard",
        vec![deposit, fee, supply, nav],
    );
    assert_eq!(out, 200_000_000);
}
