//! End-to-end test of `asm/lib/fees.masm` against the bundled
//! primitives library produced by `build.rs`.

mod common;
use common::{run, run_one};

// ----- accrue_management -------------------------------------------------------

#[test]
fn accrue_management_one_year_charges_full_fee() {
    // blocks=1000 (== year), value=10_000, fee=100 bps (1%), year=1000
    // delta = 1000 * 10000 * 100 / (10000 * 1000) = 100
    let out = run_one(
        "darwin::fees",
        "fees::accrue_management",
        vec![1_000, 10_000, 100, 1_000],
    );
    assert_eq!(out, 100);
}

#[test]
fn accrue_management_half_year_charges_half_fee() {
    let out = run_one(
        "darwin::fees",
        "fees::accrue_management",
        vec![500, 10_000, 100, 1_000],
    );
    assert_eq!(out, 50);
}

#[test]
fn accrue_management_zero_elapsed_is_zero() {
    let out = run_one(
        "darwin::fees",
        "fees::accrue_management",
        vec![0, 10_000, 100, 1_000],
    );
    assert_eq!(out, 0);
}

#[test]
fn accrue_management_zero_value_is_zero() {
    let out = run_one(
        "darwin::fees",
        "fees::accrue_management",
        vec![500, 0, 100, 1_000],
    );
    assert_eq!(out, 0);
}

#[test]
fn accrue_management_zero_fee_is_zero() {
    let out = run_one(
        "darwin::fees",
        "fees::accrue_management",
        vec![500, 10_000, 0, 1_000],
    );
    assert_eq!(out, 0);
}

#[test]
fn accrue_management_handles_realistic_scales() {
    // Miden mainnet target block time ~5s => blocks_per_year ~6.3M.
    // Use 6_000_000 here; value = 1e10 ($100M basket); fee = 100 bps;
    // elapsed = 6_000_000 (one year exactly).
    // Expected: 1% of $100M = $1M = 1e8 in our USD * 1e8 scale.
    // delta = 6_000_000 * 1e10 * 100 / (10000 * 6_000_000) = 1e8.
    let out = run_one(
        "darwin::fees",
        "fees::accrue_management",
        vec![6_000_000, 10_000_000_000, 100, 6_000_000],
    );
    assert_eq!(out, 100_000_000);
}

// ----- deduct_bps_fee ----------------------------------------------------------

#[test]
fn deduct_bps_fee_30_bps_splits_value_correctly() {
    // 100_000 * 9970 / 10000 = 99_700; fee = 300
    let out = run("darwin::fees", "fees::deduct_bps_fee", vec![100_000, 30], 2);
    assert_eq!(out[0], 99_700, "net_value");
    assert_eq!(out[1], 300, "fee_amount");
}

#[test]
fn deduct_bps_fee_zero_fee_returns_full_value() {
    let out = run("darwin::fees", "fees::deduct_bps_fee", vec![100_000, 0], 2);
    assert_eq!(out[0], 100_000);
    assert_eq!(out[1], 0);
}

#[test]
fn deduct_bps_fee_full_fee_returns_zero_net() {
    let out = run(
        "darwin::fees",
        "fees::deduct_bps_fee",
        vec![100_000, 10_000],
        2,
    );
    assert_eq!(out[0], 0);
    assert_eq!(out[1], 100_000);
}
