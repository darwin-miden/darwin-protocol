//! End-to-end test of the fee-accrual MASM in `asm/lib/fees.masm`.
//!
//! Same harness shape as the other MASM tests. Asserts both
//! procedures against hand-computed expected outputs.

use std::sync::Arc;

use miden_vm::assembly::ast::ModuleKind;
use miden_vm::assembly::{DefaultSourceManager, ModuleParser, Path, SourceManager};
use miden_vm::{
    advice::AdviceInputs, execute_sync, Assembler, DefaultHost, ExecutionOptions, StackInputs,
};

const FEES_SOURCE: &str = include_str!("../asm/lib/fees.masm");
const FEES_NAMESPACE: &str = "darwin::fees";

/// Returns the top `result_depth` elements of the stack after execution.
/// `result_depth` accounts for procedures that leave more than one
/// value on top (e.g. `deduct_bps_fee` leaves two).
fn run_with_inputs(procedure: &str, stack_inputs: Vec<u64>, result_depth: usize) -> Vec<u64> {
    assert!(result_depth >= 1);

    let push_block = stack_inputs
        .iter()
        .rev()
        .map(|v| format!("push.{v}"))
        .collect::<Vec<_>>()
        .join("\n    ");

    // After the procedure, the stack depth is 16 + result_depth. To
    // bring it back to exactly 16 we need to drop `result_depth`
    // padding zeros from below the results without touching the top
    // `result_depth` slots. `movup.15 drop` does exactly that per
    // iteration: it pulls a pad from position 15 to the top, drops it,
    // and leaves the top `result_depth` elements undisturbed (with
    // some pad reshuffling further down).
    let cleanup = (0..result_depth)
        .map(|_| "movup.15 drop".to_string())
        .collect::<Vec<_>>()
        .join("\n    ");

    let program_source = format!(
        "
use {FEES_NAMESPACE}

begin
    {push_block}
    exec.fees::{procedure}
    {cleanup}
end
"
    );

    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
    let mut parser = ModuleParser::new(ModuleKind::Library);
    let module = parser
        .parse_str(Path::new(FEES_NAMESPACE), FEES_SOURCE, source_manager)
        .expect("fees.masm parses");

    let library = Assembler::default()
        .assemble_library([module])
        .expect("fees library assembles");

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

    outputs
        .stack
        .iter()
        .take(result_depth)
        .map(|f| f.as_canonical_u64())
        .collect()
}

// ----- accrue_management -------------------------------------------------------

// Intermediate product is blocks_elapsed * basket_value * fee_bps_year,
// which must fit in u32. Tests use scaled-down values so the product
// stays below 4.29e9.

#[test]
fn accrue_management_one_year_charges_full_fee() {
    // blocks_elapsed = 1000 (== blocks_per_year)
    // basket_value   = 10_000
    // fee_bps_year   = 100 (1%)
    // blocks_per_year = 1000
    // numerator: 1000 * 10000 * 100 = 1_000_000_000  (fits in u32)
    // divisor:   10000 * 1000 = 10_000_000
    // delta:     100   (= 1% of 10000)
    let out = run_with_inputs("accrue_management", vec![1_000, 10_000, 100, 1_000], 1);
    assert_eq!(out[0], 100);
}

#[test]
fn accrue_management_half_year_charges_half_fee() {
    // 500 / 1000 blocks of the year => 50
    let out = run_with_inputs("accrue_management", vec![500, 10_000, 100, 1_000], 1);
    assert_eq!(out[0], 50);
}

#[test]
fn accrue_management_zero_elapsed_is_zero() {
    let out = run_with_inputs("accrue_management", vec![0, 10_000, 100, 1_000], 1);
    assert_eq!(out[0], 0);
}

#[test]
fn accrue_management_zero_value_is_zero() {
    let out = run_with_inputs("accrue_management", vec![500, 0, 100, 1_000], 1);
    assert_eq!(out[0], 0);
}

#[test]
fn accrue_management_zero_fee_is_zero() {
    let out = run_with_inputs("accrue_management", vec![500, 10_000, 0, 1_000], 1);
    assert_eq!(out[0], 0);
}

// ----- deduct_bps_fee ----------------------------------------------------------

#[test]
fn deduct_bps_fee_30_bps_splits_value_correctly() {
    // value=100_000, fee=30 bps
    // net = 100_000 * 9970 / 10000 = 99_700
    // fee_amount = 100_000 - 99_700 = 300
    let out = run_with_inputs("deduct_bps_fee", vec![100_000, 30], 2);
    assert_eq!(out[0], 99_700, "net_value");
    assert_eq!(out[1], 300, "fee_amount");
}

#[test]
fn deduct_bps_fee_zero_fee_returns_full_value() {
    let out = run_with_inputs("deduct_bps_fee", vec![100_000, 0], 2);
    assert_eq!(out[0], 100_000);
    assert_eq!(out[1], 0);
}

#[test]
fn deduct_bps_fee_full_fee_returns_zero_net() {
    let out = run_with_inputs("deduct_bps_fee", vec![100_000, 10_000], 2);
    assert_eq!(out[0], 0);
    assert_eq!(out[1], 100_000);
}
