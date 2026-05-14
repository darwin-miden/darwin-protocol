//! Shared test helpers for the MASM library tests.
//!
//! Each test file builds a small probe program that pushes inputs,
//! exec's one of the `darwin::*` library procedures, and reads the
//! resulting top-of-stack. The helpers below centralise the assembler
//! wiring (Darwin primitives + flow + miden-core-lib) and the stack
//! cleanup so the per-test code stays tight.

#![allow(dead_code)]

use miden_vm::{
    advice::AdviceInputs, execute_sync, Assembler, DefaultHost, ExecutionOptions, StackInputs,
};

/// Runs `library_path::procedure` on `inputs` (pushed in reverse order
/// so the first input ends up on top), returning the top
/// `result_depth` elements of the output stack.
///
/// The probe program attaches:
///   - `miden-core-lib` (for `miden::core::math::u64::div` and the
///     associated event handlers required by `darwin::math::felt_div`)
///   - the bundled `darwin::*` primitives library (math, nav, mint,
///     fees, redeem)
///   - the bundled `darwin::flow` library (the higher-level
///     compositions in `asm/lib/flow.masm`)
pub fn run(library_path: &str, procedure: &str, inputs: Vec<u64>, result_depth: usize) -> Vec<u64> {
    assert!(result_depth >= 1, "result_depth must be >= 1");

    let push_block = inputs
        .iter()
        .rev()
        .map(|v| format!("push.{v}"))
        .collect::<Vec<_>>()
        .join("\n    ");
    let cleanup = (0..result_depth)
        .map(|_| "movup.15 drop".to_string())
        .collect::<Vec<_>>()
        .join("\n    ");

    let program_source = format!(
        "
use {library_path}

begin
    {push_block}
    exec.{procedure}
    {cleanup}
end
"
    );

    let core_library = miden_core_lib::CoreLibrary::default();
    let primitives = darwin_protocol_account::primitives_library();
    let flow = darwin_protocol_account::flow_library();

    let program = Assembler::default()
        .with_static_library(core_library.as_ref())
        .expect("core library attaches")
        .with_static_library(&primitives)
        .expect("primitives attaches")
        .with_static_library(&flow)
        .expect("flow attaches")
        .assemble_program(program_source.as_str())
        .expect("probe program assembles");

    let mut host = DefaultHost::default()
        .with_library(&core_library)
        .expect("core library handlers register with the host");

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

/// Convenience wrapper for procedures that leave one element on top.
pub fn run_one(library_path: &str, procedure: &str, inputs: Vec<u64>) -> u64 {
    run(library_path, procedure, inputs, 1)[0]
}
