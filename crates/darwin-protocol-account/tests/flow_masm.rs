//! Integration test for `asm/lib/flow.masm` against the bundled
//! primitives + flow libraries produced by `build.rs`.

mod common;
use common::run_one;

// ----- mint flows --------------------------------------------------------------

#[test]
fn mint_for_3_asset_deposit_matches_per_step_computation() {
    // (2,10) + (3,5) + (1,30) = 20 + 15 + 30 = 65.
    // supply=100, nav=10, fee=0  =>  mint = 65 * 10000 * 100 / (10000 * 10) = 650.
    let out = run_one(
        "darwin::flow",
        "flow::mint_amount_for_3_asset_deposit",
        vec![2, 10, 3, 5, 1, 30, 0, 100, 10],
    );
    assert_eq!(out, 650);
}

#[test]
fn mint_for_3_asset_deposit_with_fee_takes_fee_off_the_top() {
    // fee=30 bps => mint = 65 * 9970 * 100 / (10000 * 10) = 648 (truncated)
    let out = run_one(
        "darwin::flow",
        "flow::mint_amount_for_3_asset_deposit",
        vec![2, 10, 3, 5, 1, 30, 30, 100, 10],
    );
    assert_eq!(out, 648);
}

#[test]
fn mint_for_2_asset_deposit_matches_aggressive_basket_shape() {
    let out = run_one(
        "darwin::flow",
        "flow::mint_amount_for_2_asset_deposit",
        vec![2, 10, 3, 5, 0, 70, 10],
    );
    assert_eq!(out, 245);
}

#[test]
fn mint_for_4_asset_deposit_matches_conservative_basket_shape() {
    let out = run_one(
        "darwin::flow",
        "flow::mint_amount_for_4_asset_deposit",
        vec![1, 1, 2, 2, 3, 3, 4, 4, 0, 300, 10],
    );
    assert_eq!(out, 900);
}

#[test]
fn mint_for_3_asset_deposit_within_felt_intermediate_budget() {
    // The full mint formula is
    //     mint = deposit_value * (10000 - fee_bps) * supply / (10000 * nav)
    // The intermediate product `deposit_value * 10000 * supply` is
    // computed in felt arithmetic, and must stay below the Goldilocks
    // modulus (~1.84e19) to avoid wrap-around.
    //
    // Inputs:
    //   prices  = (200, 300, 100), amounts = (10_000, 10_000, 10_000)
    //   => deposit_value = 6_000_000
    //   pre_supply = 1_000_000  (1M basket tokens)
    //   pre_nav    = 500_000    (NAV per share = 0.5 in the spec scale)
    //   fee_bps    = 0
    //
    // Intermediate budget check: 6e6 * 1e4 * 1e6 = 6e16, well under 1.84e19.
    // mint = 6_000_000 * 10000 * 1_000_000 / (10000 * 500_000) = 12_000_000.
    let out = run_one(
        "darwin::flow",
        "flow::mint_amount_for_3_asset_deposit",
        vec![200, 10_000, 300, 10_000, 100, 10_000, 0, 1_000_000, 500_000],
    );
    assert_eq!(out, 12_000_000);
}

// ----- release flow ------------------------------------------------------------

#[test]
fn release_for_constituent_with_30_bps_fee_yields_correct_amount() {
    // net = 200 * 9970 / 10000 = 199 (truncated)
    // release = 199 * 5000 / (10000 * 2) = 49 (truncated)
    let out = run_one(
        "darwin::flow",
        "flow::release_amount_for_constituent",
        vec![200, 30, 5000, 2],
    );
    assert_eq!(out, 49);
}

#[test]
fn release_for_constituent_with_zero_fee_at_par_returns_value() {
    let out = run_one(
        "darwin::flow",
        "flow::release_amount_for_constituent",
        vec![100, 0, 10_000, 1],
    );
    assert_eq!(out, 100);
}
