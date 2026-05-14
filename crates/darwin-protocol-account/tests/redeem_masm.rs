//! End-to-end test of `asm/lib/redeem.masm` against the bundled
//! primitives library produced by `build.rs`.

mod common;
use common::run_one;

// ----- redeem_value_usd --------------------------------------------------------

#[test]
fn redeem_value_at_par_returns_burn_amount() {
    let out = run_one(
        "darwin::redeem",
        "redeem::redeem_value_usd",
        vec![100, 50, 50],
    );
    assert_eq!(out, 100);
}

#[test]
fn redeem_value_doubles_when_nav_doubles() {
    let out = run_one(
        "darwin::redeem",
        "redeem::redeem_value_usd",
        vec![100, 100, 50],
    );
    assert_eq!(out, 200);
}

#[test]
fn redeem_value_halves_when_nav_halves() {
    let out = run_one(
        "darwin::redeem",
        "redeem::redeem_value_usd",
        vec![100, 25, 50],
    );
    assert_eq!(out, 50);
}

#[test]
fn redeem_value_zero_burn_is_zero() {
    let out = run_one(
        "darwin::redeem",
        "redeem::redeem_value_usd",
        vec![0, 100, 50],
    );
    assert_eq!(out, 0);
}

#[test]
fn redeem_value_handles_u64_range() {
    // burn=1e10 base units, nav=1e8 ($1), supply=1e12 (basket has $10k)
    // value = 1e10 * 1e8 / 1e12 = 1e6
    let out = run_one(
        "darwin::redeem",
        "redeem::redeem_value_usd",
        vec![10_000_000_000, 100_000_000, 1_000_000_000_000],
    );
    assert_eq!(out, 1_000_000);
}

// ----- release_amount ----------------------------------------------------------

#[test]
fn release_amount_50_percent_weight_at_par_price_returns_half_value() {
    let out = run_one(
        "darwin::redeem",
        "redeem::release_amount",
        vec![200, 5000, 1],
    );
    assert_eq!(out, 100);
}

#[test]
fn release_amount_full_weight_returns_value_over_price() {
    let out = run_one(
        "darwin::redeem",
        "redeem::release_amount",
        vec![200, 10_000, 4],
    );
    assert_eq!(out, 50);
}

#[test]
fn release_amount_zero_weight_returns_zero() {
    let out = run_one("darwin::redeem", "redeem::release_amount", vec![200, 0, 5]);
    assert_eq!(out, 0);
}

#[test]
fn release_amount_scales_inverse_with_price() {
    let cheap = run_one(
        "darwin::redeem",
        "redeem::release_amount",
        vec![1000, 5000, 10],
    );
    let dear = run_one(
        "darwin::redeem",
        "redeem::release_amount",
        vec![1000, 5000, 20],
    );
    assert_eq!(cheap, 50);
    assert_eq!(dear, 25);
}
