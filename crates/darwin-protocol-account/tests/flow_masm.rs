//! Integration test for `asm/lib/flow.masm`: composes the four math
//! library modules (nav, mint, fees, redeem) into Flow A / Flow C
//! wrappers and exercises end-to-end mint / release scenarios.
//!
//! This test proves the libraries are *composable* — each module can
//! be imported and called from another module in the same protocol
//! library.

use std::sync::Arc;

use miden_vm::assembly::ast::ModuleKind;
use miden_vm::assembly::{DefaultSourceManager, ModuleParser, Path, SourceManager};
use miden_vm::{
    advice::AdviceInputs, execute_sync, Assembler, DefaultHost, ExecutionOptions, StackInputs,
};

const NAV_SOURCE: &str = include_str!("../asm/lib/nav.masm");
const MINT_SOURCE: &str = include_str!("../asm/lib/mint.masm");
const FEES_SOURCE: &str = include_str!("../asm/lib/fees.masm");
const REDEEM_SOURCE: &str = include_str!("../asm/lib/redeem.masm");
const FLOW_SOURCE: &str = include_str!("../asm/lib/flow.masm");

fn parse_module(
    namespace: &str,
    source: &str,
    source_manager: Arc<dyn SourceManager>,
) -> Box<miden_vm::assembly::ast::Module> {
    let mut parser = ModuleParser::new(ModuleKind::Library);
    parser
        .parse_str(Path::new(namespace), source, source_manager)
        .unwrap_or_else(|e| panic!("module {namespace} should parse cleanly: {e}"))
}

fn run_with_inputs(procedure: &str, stack_inputs: Vec<u64>) -> Vec<u64> {
    let push_block = stack_inputs
        .iter()
        .rev()
        .map(|v| format!("push.{v}"))
        .collect::<Vec<_>>()
        .join("\n    ");

    let program_source = format!(
        "
use darwin::flow

begin
    {push_block}
    exec.flow::{procedure}
    swap drop
end
"
    );

    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());

    // Assemble the four math modules into a single primitives library.
    let primitives = Assembler::default()
        .assemble_library([
            parse_module("darwin::nav", NAV_SOURCE, source_manager.clone()),
            parse_module("darwin::mint", MINT_SOURCE, source_manager.clone()),
            parse_module("darwin::fees", FEES_SOURCE, source_manager.clone()),
            parse_module("darwin::redeem", REDEEM_SOURCE, source_manager.clone()),
        ])
        .expect("primitives library assembles");

    // Assemble the flow library with the primitives library attached.
    let flow_library = Assembler::default()
        .with_static_library(&primitives)
        .expect("primitives attaches to flow assembler")
        .assemble_library([parse_module("darwin::flow", FLOW_SOURCE, source_manager)])
        .expect("flow library assembles");

    // Build the probe program with both libraries attached.
    let program = Assembler::default()
        .with_static_library(&primitives)
        .expect("primitives attaches to program assembler")
        .with_static_library(&flow_library)
        .expect("flow attaches to program assembler")
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

// ----- mint flows --------------------------------------------------------------

#[test]
fn mint_for_3_asset_deposit_matches_per_step_computation() {
    // Three constituents: (price=2,amount=10), (price=3,amount=5),
    // (price=1,amount=30).
    // deposit_value = 2*10 + 3*5 + 1*30 = 20 + 15 + 30 = 65.
    // pre_supply=100, pre_nav=10, fee=0.
    // mint = 65 * 10000 * 100 / (10000 * 10) = 650.
    let out = run_with_inputs(
        "mint_amount_for_3_asset_deposit",
        vec![2, 10, 3, 5, 1, 30, 0, 100, 10],
    );
    assert_eq!(out[0], 650);
}

#[test]
fn mint_for_3_asset_deposit_with_fee_takes_fee_off_the_top() {
    // Same deposit as above, fee=30 bps.
    // deposit_value=65, net=65*9970/10000 (integer division)... but it's
    // not integer-truncated before multiplying through. The composed
    // formula gives:
    //   mint = 65 * 9970 * 100 / (10000 * 10) = 64805
    // Integer-divided by (10000 * 10 = 100_000): 64805000 / 100000 = 648
    let out = run_with_inputs(
        "mint_amount_for_3_asset_deposit",
        vec![2, 10, 3, 5, 1, 30, 30, 100, 10],
    );
    assert_eq!(out[0], 648);
}

#[test]
fn mint_for_2_asset_deposit_matches_aggressive_basket_shape() {
    // (p=2,q=10) + (p=3,q=5) = 35. supply=70, nav=10, fee=0.
    // mint = 35 * 10000 * 70 / (10000 * 10) = 245
    let out = run_with_inputs(
        "mint_amount_for_2_asset_deposit",
        vec![2, 10, 3, 5, 0, 70, 10],
    );
    assert_eq!(out[0], 245);
}

#[test]
fn mint_for_4_asset_deposit_matches_conservative_basket_shape() {
    // Four (p,q): (1,1), (2,2), (3,3), (4,4). Sum = 1+4+9+16 = 30.
    // pre_supply=300, pre_nav=10, fee=0.
    // mint = 30 * 10000 * 300 / (10000 * 10) = 900
    let out = run_with_inputs(
        "mint_amount_for_4_asset_deposit",
        vec![1, 1, 2, 2, 3, 3, 4, 4, 0, 300, 10],
    );
    assert_eq!(out[0], 900);
}

// ----- release flow ------------------------------------------------------------

#[test]
fn release_for_constituent_with_30_bps_fee_yields_correct_amount() {
    // redeem_value = 200, redeem_fee = 30 bps, weight = 5000 (50%),
    // price = 2.
    // net_value = 200 * 9970 / 10000 = 199 (u32div truncation)
    // release = 199 * 5000 / (10000 * 2) = 49 (u32div truncation)
    let out = run_with_inputs("release_amount_for_constituent", vec![200, 30, 5000, 2]);
    assert_eq!(out[0], 49);
}

#[test]
fn release_for_constituent_with_zero_fee_at_par_returns_value() {
    // redeem_value=100, fee=0, weight=10000 (100%), price=1
    // net = 100, release = 100*10000/(10000*1) = 100
    let out = run_with_inputs("release_amount_for_constituent", vec![100, 0, 10_000, 1]);
    assert_eq!(out[0], 100);
}
