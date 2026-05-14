//! End-to-end test of `asm/lib/drift.masm` against the bundled
//! primitives library produced by `build.rs`. Same harness as the
//! other MASM tests.

mod common;
use common::run_one;

// ----- constituent_weight_bps -------------------------------------------------

#[test]
fn constituent_weight_at_par_returns_target_bps() {
    // position * price = 4000, total = 10000  =>  4000*10000/10000 = 4000
    let out = run_one(
        "darwin::drift",
        "drift::constituent_weight_bps",
        vec![100, 40, 10_000],
    );
    assert_eq!(out, 4000);
}

#[test]
fn constituent_weight_handles_zero_position() {
    let out = run_one(
        "darwin::drift",
        "drift::constituent_weight_bps",
        vec![0, 40, 10_000],
    );
    assert_eq!(out, 0);
}

#[test]
fn constituent_weight_handles_full_basket() {
    // position * price equals the whole pool  =>  weight = 10000 bps.
    let out = run_one(
        "darwin::drift",
        "drift::constituent_weight_bps",
        vec![100, 100, 10_000],
    );
    assert_eq!(out, 10_000);
}

// ----- abs_drift_bps -----------------------------------------------------------

#[test]
fn abs_drift_when_current_exceeds_target() {
    // current = 4500, target = 4000  =>  drift = 500
    let out = run_one("darwin::drift", "drift::abs_drift_bps", vec![4500, 4000]);
    assert_eq!(out, 500);
}

#[test]
fn abs_drift_when_target_exceeds_current() {
    // current = 3500, target = 4000  =>  drift = 500
    let out = run_one("darwin::drift", "drift::abs_drift_bps", vec![3500, 4000]);
    assert_eq!(out, 500);
}

#[test]
fn abs_drift_when_equal_returns_zero() {
    let out = run_one("darwin::drift", "drift::abs_drift_bps", vec![4000, 4000]);
    assert_eq!(out, 0);
}

// ----- needs_rebalance ---------------------------------------------------------

#[test]
fn needs_rebalance_above_threshold() {
    // drift = 600, threshold = 500  =>  1
    let out = run_one("darwin::drift", "drift::needs_rebalance", vec![600, 500]);
    assert_eq!(out, 1);
}

#[test]
fn needs_rebalance_below_threshold() {
    // drift = 400, threshold = 500  =>  0
    let out = run_one("darwin::drift", "drift::needs_rebalance", vec![400, 500]);
    assert_eq!(out, 0);
}

#[test]
fn needs_rebalance_at_threshold_is_zero() {
    // strict inequality — at exactly the threshold no rebalance fires.
    let out = run_one("darwin::drift", "drift::needs_rebalance", vec![500, 500]);
    assert_eq!(out, 0);
}
