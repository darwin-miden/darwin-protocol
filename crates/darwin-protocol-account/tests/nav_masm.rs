//! End-to-end test of the NAV-primitive MASM in `asm/lib/nav.masm`.
//!
//! Parses + assembles the source as the `darwin::nav` library, runs
//! each procedure via miden-vm, and asserts the resulting stack against
//! the spec formula.
//!
//! When this test passes, we know the Miden toolchain is wired
//! correctly all the way from MASM source to verified execution.

use std::sync::Arc;

use miden_vm::assembly::ast::ModuleKind;
use miden_vm::assembly::{DefaultSourceManager, ModuleParser, Path, SourceManager};
use miden_vm::{
    advice::AdviceInputs, execute_sync, Assembler, DefaultHost, ExecutionOptions, StackInputs,
};

const NAV_SOURCE: &str = include_str!("../asm/lib/nav.masm");
const NAV_NAMESPACE: &str = "darwin::nav";

fn run_with_inputs(procedure: &str, stack_inputs: Vec<u64>) -> Vec<u64> {
    // Build the program source that pushes the inputs and calls the
    // procedure under test. The stack is LIFO — reverse the inputs so
    // the first one ends up on top.
    let push_block = stack_inputs
        .iter()
        .rev()
        .map(|v| format!("push.{v}"))
        .collect::<Vec<_>>()
        .join("\n    ");

    // Miden requires the program to end with exactly 16 stack elements.
    // Each procedure leaves a single result on top, growing the depth
    // by 1 net (16 implicit zeros + N pushed inputs - N consumed
    // inputs + 1 result = 17). Bring a zero to the top and drop it
    // so the result stays at index 0 and the depth returns to 16.
    let program_source = format!(
        "
use {NAV_NAMESPACE}

begin
    {push_block}
    exec.nav::{procedure}
    swap drop
end
"
    );

    // Parse the nav.masm source as a library module under the
    // `darwin::nav` namespace so the program can `use` it.
    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());
    let mut parser = ModuleParser::new(ModuleKind::Library);
    let nav_module = parser
        .parse_str(Path::new(NAV_NAMESPACE), NAV_SOURCE, source_manager)
        .expect("nav.masm parses cleanly");

    let library = Assembler::default()
        .assemble_library([nav_module])
        .expect("nav library assembles");

    let program = Assembler::default()
        .with_static_library(&library)
        .expect("library can be attached to the assembler")
        .assemble_program(program_source.as_str())
        .expect("program assembles");

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

#[test]
fn weighted_sum_2_matches_hand_computation() {
    // p1=200, q1=3, p2=50, q2=4   =>  200*3 + 50*4 = 800
    let out = run_with_inputs("weighted_sum_2", vec![200, 3, 50, 4]);
    assert_eq!(out[0], 800);
}

#[test]
fn weighted_sum_3_matches_hand_computation() {
    // 100*2 + 300*5 + 7*11 = 200 + 1500 + 77 = 1777
    let out = run_with_inputs("weighted_sum_3", vec![100, 2, 300, 5, 7, 11]);
    assert_eq!(out[0], 1777);
}

#[test]
fn weighted_sum_4_matches_hand_computation() {
    // 1*1 + 2*2 + 3*3 + 4*4 = 30
    let out = run_with_inputs("weighted_sum_4", vec![1, 1, 2, 2, 3, 3, 4, 4]);
    assert_eq!(out[0], 30);
}

#[test]
fn nav_per_share_integer_division() {
    // Both operands fit in u32. sum=100_000_000, supply=1_000_000 => 100.
    let out = run_with_inputs("nav_per_share", vec![100_000_000, 1_000_000]);
    assert_eq!(out[0], 100);
}

#[test]
fn nav_per_share_truncates_toward_zero() {
    // 7 / 3 = 2 (integer division)
    let out = run_with_inputs("nav_per_share", vec![7, 3]);
    assert_eq!(out[0], 2);
}
