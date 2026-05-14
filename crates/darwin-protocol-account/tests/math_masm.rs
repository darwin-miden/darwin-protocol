//! End-to-end test of `asm/lib/math.masm::felt_div`.
//!
//! Unlike the other MASM tests, this one *also* attaches the
//! miden-core-lib (which provides `miden::core::math::u64::div`) so
//! the divider can be wired into the assembler. Once the core library
//! is on the assembler, the bundled `darwin::math` library loads
//! cleanly and we can run real u64-range divisions through it.

use std::sync::Arc;

use miden_vm::assembly::ast::ModuleKind;
use miden_vm::assembly::{DefaultSourceManager, ModuleParser, Path, SourceManager};
use miden_vm::{
    advice::AdviceInputs, execute_sync, Assembler, DefaultHost, ExecutionOptions, StackInputs,
};

fn run(num: u64, div: u64) -> u64 {
    let program_source = format!(
        "
use darwin::math

begin
    push.{div}
    push.{num}
    exec.math::felt_div
    swap drop
end
"
    );

    let core_library = miden_core_lib::CoreLibrary::default();

    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());

    // Re-assemble the math module against the core library so we can
    // attach a self-contained pair of libraries to the program.
    let math_source = darwin_protocol_account::MATH_MASM;
    let mut parser = ModuleParser::new(ModuleKind::Library);
    let math_module = parser
        .parse_str(Path::new("darwin::math"), math_source, source_manager)
        .expect("math.masm parses");

    let math_lib = Assembler::default()
        .with_static_library(core_library.as_ref())
        .expect("core library attaches to math assembler")
        .assemble_library([math_module])
        .expect("math library assembles");

    let program = Assembler::default()
        .with_static_library(core_library.as_ref())
        .expect("core library attaches to program assembler")
        .with_static_library(&math_lib)
        .expect("math attaches to program assembler")
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

    outputs.stack[0].as_canonical_u64()
}

#[test]
fn felt_div_small_values_match_u32_arithmetic() {
    assert_eq!(run(7, 3), 2);
    assert_eq!(run(100_000_000, 1_000_000), 100);
    assert_eq!(run(1, 1), 1);
    assert_eq!(run(0, 1_000_000), 0);
}

#[test]
fn felt_div_works_past_u32_max() {
    // Both operands above 2^32 = 4_294_967_296.
    // 10_000_000_000 / 100_000_000 = 100
    assert_eq!(run(10_000_000_000, 100_000_000), 100);

    // 1e18 / 1e8 = 1e10 — quotient itself doesn't fit in u32 either.
    assert_eq!(run(1_000_000_000_000_000_000, 100_000_000), 10_000_000_000);
}

#[test]
fn felt_div_truncates_toward_zero() {
    assert_eq!(run(999, 1000), 0);
    assert_eq!(run(1_000_000_000_001, 1_000_000_000_000), 1);
}
