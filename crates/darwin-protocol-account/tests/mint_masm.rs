//! End-to-end test of the mint-formula MASM in `asm/lib/mint.masm`.
//!
//! Same harness shape as `nav_masm.rs`: parse → assemble library →
//! link into a probe program → execute → assert top of stack.
//!
//! Both `par_value` and `standard` use `u32div` for the final
//! division, so the test inputs stay below u32 max (`~4.29e9`). The
//! spec's §6.3 acknowledges this trade-off; a u64-safe rewrite is
//! tracked as a follow-up in `asm/lib/mint.masm`.

use std::sync::Arc;

use miden_vm::assembly::ast::ModuleKind;
use miden_vm::assembly::{DefaultSourceManager, ModuleParser, Path, SourceManager};
use miden_vm::{
    advice::AdviceInputs, execute_sync, Assembler, DefaultHost, ExecutionOptions, StackInputs,
};

const MINT_SOURCE: &str = include_str!("../asm/lib/mint.masm");
const MINT_NAMESPACE: &str = "darwin::mint";

fn run_with_inputs(procedure: &str, stack_inputs: Vec<u64>) -> Vec<u64> {
    let push_block = stack_inputs
        .iter()
        .rev()
        .map(|v| format!("push.{v}"))
        .collect::<Vec<_>>()
        .join("\n    ");

    let program_source = format!(
        "
use {MINT_NAMESPACE}

begin
    {push_block}
    exec.mint::{procedure}
    swap drop
end
"
    );

    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
    let mut parser = ModuleParser::new(ModuleKind::Library);
    let mint_module = parser
        .parse_str(Path::new(MINT_NAMESPACE), MINT_SOURCE, source_manager)
        .expect("mint.masm parses cleanly");

    let library = Assembler::default()
        .assemble_library([mint_module])
        .expect("mint library assembles");

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

// ----- par_value ---------------------------------------------------------------

#[test]
fn par_value_no_fee_returns_deposit_value() {
    // fee_bps = 0  =>  mint_amount = deposit_value
    let out = run_with_inputs("par_value", vec![100_000, 0]);
    assert_eq!(out[0], 100_000);
}

#[test]
fn par_value_with_30_bps_fee_keeps_99_70_percent() {
    // 100_000 * 9970 / 10000 = 99_700
    let out = run_with_inputs("par_value", vec![100_000, 30]);
    assert_eq!(out[0], 99_700);
}

#[test]
fn par_value_with_full_fee_returns_zero() {
    // fee_bps = 10000 (100%)  =>  mint_amount = 0
    let out = run_with_inputs("par_value", vec![100_000, 10_000]);
    assert_eq!(out[0], 0);
}

// ----- standard ----------------------------------------------------------------

// The standard procedure's intermediate product
// `deposit * (10000 - fee) * supply` overflows u32 for any combination
// where the three factors collectively exceed 2^32. The tests below
// pick small values that stay below that limit while still exercising
// the formula's three regimes (par-state, NAV<1, NAV>1).

#[test]
fn standard_no_fee_par_state_returns_deposit_value() {
    // deposit=100, supply=100, nav=100, fee=0
    // mint = 100 * 10000 * 100 / (10000 * 100) = 100
    // intermediate numerator: 100 * 10000 * 100 = 100_000_000 (fits in u32)
    let out = run_with_inputs("standard", vec![100, 0, 100, 100]);
    assert_eq!(out[0], 100);
}

#[test]
fn standard_with_30_bps_fee_at_par_returns_fee_adjusted_value() {
    // deposit=100, supply=100, nav=100, fee=30 bps
    // mint = 100 * 9970 * 100 / (10000 * 100) = 99
    // (integer-divided from 99.7 — `u32div` truncates toward zero.)
    let out = run_with_inputs("standard", vec![100, 30, 100, 100]);
    assert_eq!(out[0], 99);
}

#[test]
fn standard_doubles_supply_when_nav_is_half() {
    // deposit=100, supply=100, nav=50, fee=0
    // mint = 100 * 10000 * 100 / (10000 * 50) = 200
    let out = run_with_inputs("standard", vec![100, 0, 100, 50]);
    assert_eq!(out[0], 200);
}

#[test]
fn standard_halves_supply_when_nav_is_double() {
    // deposit=100, supply=100, nav=200, fee=0
    // mint = 100 * 10000 * 100 / (10000 * 200) = 50
    let out = run_with_inputs("standard", vec![100, 0, 100, 200]);
    assert_eq!(out[0], 50);
}
