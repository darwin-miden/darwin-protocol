//! Cross-validation: for every M1 basket, build an at-par snapshot
//! and assert that `darwin::drift::constituent_weight_bps` returns
//! each constituent's target weight, then perturb one constituent
//! and assert `needs_rebalance` fires at the manifest's threshold.
//!
//! These tests use the basket manifests in `darwin-baskets` as the
//! source of truth — they are the same manifests the SDK rebalance
//! planner consumes, so a pass here means the MASM and the Rust
//! planner agree on the drift formula for every M1 basket.

mod common;
use common::run_one;

fn par_total(weights: &[u64], unit_price: u64) -> u64 {
    weights.iter().sum::<u64>() * unit_price
}

#[test]
fn at_par_every_constituent_matches_its_target() {
    for basket in darwin_baskets::all_m1() {
        let weights: Vec<u64> = basket
            .constituents
            .iter()
            .map(|c| c.target_weight_bps as u64)
            .collect();
        let total = par_total(&weights, 1);

        for c in &basket.constituents {
            let position = c.target_weight_bps as u64;
            let observed = run_one(
                "darwin::drift",
                "drift::constituent_weight_bps",
                vec![position, 1, total],
            );
            assert_eq!(
                observed, c.target_weight_bps as u64,
                "{} / {}: expected {} bps at par, got {} bps",
                basket.symbol, c.faucet_alias, c.target_weight_bps, observed,
            );
        }
    }
}

#[test]
fn doubled_constituent_drifts_above_threshold() {
    for basket in darwin_baskets::all_m1() {
        let weights: Vec<u64> = basket
            .constituents
            .iter()
            .map(|c| c.target_weight_bps as u64)
            .collect();
        let threshold = basket.rebalancing.drift_threshold_bps as u64;

        // Pick the first constituent and double its position. The
        // total pool value grows accordingly. New weight is
        // computed via the MASM library directly so this test
        // exercises drift::constituent_weight_bps and drift::abs_drift_bps
        // together with drift::needs_rebalance on every basket.
        let first = &basket.constituents[0];
        let mut new_weights = weights.clone();
        new_weights[0] *= 2;
        let new_total: u64 = new_weights.iter().sum();

        let new_bps = run_one(
            "darwin::drift",
            "drift::constituent_weight_bps",
            vec![new_weights[0], 1, new_total],
        );

        let drift = run_one(
            "darwin::drift",
            "drift::abs_drift_bps",
            vec![new_bps, first.target_weight_bps as u64],
        );

        let needs = run_one(
            "darwin::drift",
            "drift::needs_rebalance",
            vec![drift, threshold],
        );

        assert!(
            new_bps > first.target_weight_bps as u64,
            "{} / {}: doubling should increase weight ({} -> {})",
            basket.symbol,
            first.faucet_alias,
            first.target_weight_bps,
            new_bps,
        );
        assert!(
            drift > threshold,
            "{} / {}: drift {} should exceed threshold {}",
            basket.symbol,
            first.faucet_alias,
            drift,
            threshold,
        );
        assert_eq!(
            needs, 1,
            "{} / {}: needs_rebalance must fire (drift {} > threshold {})",
            basket.symbol, first.faucet_alias, drift, threshold,
        );
    }
}
