//! End-to-end test of the redeem-math MASM in `asm/lib/redeem.masm`.
//!
//! Same harness shape as the other MASM tests. Asserts both
//! procedures against hand-computed expected outputs.

use std::sync::Arc;

use miden_vm::assembly::ast::ModuleKind;
use miden_vm::assembly::{DefaultSourceManager, ModuleParser, Path, SourceManager};
use miden_vm::{
    advice::AdviceInputs, execute_sync, Assembler, DefaultHost, ExecutionOptions, StackInputs,
};

const REDEEM_SOURCE: &str = include_str!("../asm/lib/redeem.masm");
const REDEEM_NAMESPACE: &str = "darwin::redeem";

fn run_with_inputs(procedure: &str, stack_inputs: Vec<u64>) -> Vec<u64> {
    let push_block = stack_inputs
        .iter()
        .rev()
        .map(|v| format!("push.{v}"))
        .collect::<Vec<_>>()
        .join("\n    ");

    let program_source = format!(
        "
use {REDEEM_NAMESPACE}

begin
    {push_block}
    exec.redeem::{procedure}
    swap drop
end
"
    );

    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
    let mut parser = ModuleParser::new(ModuleKind::Library);
    let module = parser
        .parse_str(Path::new(REDEEM_NAMESPACE), REDEEM_SOURCE, source_manager)
        .expect("redeem.masm parses");

    let library = Assembler::default()
        .assemble_library([module])
        .expect("redeem library assembles");

    let program = Assembler::default()
        .with_static_library(&library)
        .expect("library attaches")
        .assemble_program(program_source.as_str())
        .expect("probe program assembles");

    let mut host = DefaultHost::default();
    let outputs = execute_sync(
        &program,
        StackInputs::default(),
        AdviceInputs::default(),
        &mut host,
        ExecutionOptions::default(),
    )
    .expect("program executes");

    outputs.stack.iter().map(|f| f.as_canonical_u64()).collect()
}

// ----- redeem_value_usd --------------------------------------------------------

#[test]
fn redeem_value_at_par_returns_burn_amount() {
    // burn = 100, nav = 50, supply = 50  =>  100 * 50 / 50 = 100
    let out = run_with_inputs("redeem_value_usd", vec![100, 50, 50]);
    assert_eq!(out[0], 100);
}

#[test]
fn redeem_value_doubles_when_nav_doubles() {
    // burn = 100, nav = 100, supply = 50  =>  100 * 100 / 50 = 200
    let out = run_with_inputs("redeem_value_usd", vec![100, 100, 50]);
    assert_eq!(out[0], 200);
}

#[test]
fn redeem_value_halves_when_nav_halves() {
    // burn = 100, nav = 25, supply = 50  =>  100 * 25 / 50 = 50
    let out = run_with_inputs("redeem_value_usd", vec![100, 25, 50]);
    assert_eq!(out[0], 50);
}

#[test]
fn redeem_value_zero_burn_is_zero() {
    let out = run_with_inputs("redeem_value_usd", vec![0, 100, 50]);
    assert_eq!(out[0], 0);
}

// ----- release_amount ----------------------------------------------------------

#[test]
fn release_amount_50_percent_weight_at_par_price_returns_half_value() {
    // net_value = 200, weight = 5000 bps (50%), price = 1
    // release = 200 * 5000 / (10000 * 1) = 100
    let out = run_with_inputs("release_amount", vec![200, 5000, 1]);
    assert_eq!(out[0], 100);
}

#[test]
fn release_amount_full_weight_returns_value_over_price() {
    // net_value = 200, weight = 10000 bps (100%), price = 4
    // release = 200 * 10000 / (10000 * 4) = 50
    let out = run_with_inputs("release_amount", vec![200, 10_000, 4]);
    assert_eq!(out[0], 50);
}

#[test]
fn release_amount_zero_weight_returns_zero() {
    let out = run_with_inputs("release_amount", vec![200, 0, 5]);
    assert_eq!(out[0], 0);
}

#[test]
fn release_amount_scales_inverse_with_price() {
    // Doubling the price halves the released amount (same USD value
    // bought twice as expensive a unit).
    let net = 1000;
    let weight = 5000;
    let out_cheap = run_with_inputs("release_amount", vec![net, weight, 10]);
    let out_dear = run_with_inputs("release_amount", vec![net, weight, 20]);
    assert_eq!(out_cheap[0], 50);
    assert_eq!(out_dear[0], 25);
}
