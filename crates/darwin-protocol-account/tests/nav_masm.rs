//! End-to-end test of `asm/lib/nav.masm` against the bundled
//! primitives library produced by `build.rs`.

mod common;
use common::run_one;

#[test]
fn weighted_sum_2_matches_hand_computation() {
    // p1=200, q1=3, p2=50, q2=4   =>  200*3 + 50*4 = 800
    let out = run_one("darwin::nav", "nav::weighted_sum_2", vec![200, 3, 50, 4]);
    assert_eq!(out, 800);
}

#[test]
fn weighted_sum_3_matches_hand_computation() {
    // 100*2 + 300*5 + 7*11 = 200 + 1500 + 77 = 1777
    let out = run_one(
        "darwin::nav",
        "nav::weighted_sum_3",
        vec![100, 2, 300, 5, 7, 11],
    );
    assert_eq!(out, 1777);
}

#[test]
fn weighted_sum_4_matches_hand_computation() {
    // 1*1 + 2*2 + 3*3 + 4*4 = 1 + 4 + 9 + 16 = 30
    let out = run_one(
        "darwin::nav",
        "nav::weighted_sum_4",
        vec![1, 1, 2, 2, 3, 3, 4, 4],
    );
    assert_eq!(out, 30);
}

#[test]
fn nav_per_share_small_values() {
    // 100_000_000 / 1_000_000 = 100
    let out = run_one(
        "darwin::nav",
        "nav::nav_per_share",
        vec![100_000_000, 1_000_000],
    );
    assert_eq!(out, 100);
}

#[test]
fn nav_per_share_truncates_toward_zero() {
    // 7 / 3 = 2 (integer division)
    let out = run_one("darwin::nav", "nav::nav_per_share", vec![7, 3]);
    assert_eq!(out, 2);
}

#[test]
fn nav_per_share_handles_u64_range_operands() {
    // Both inputs exceed u32 max — felt_div under the hood handles them.
    // 1e18 / 1e8 = 1e10
    let out = run_one(
        "darwin::nav",
        "nav::nav_per_share",
        vec![1_000_000_000_000_000_000, 100_000_000],
    );
    assert_eq!(out, 10_000_000_000);
}
