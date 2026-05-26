//! Sanity-check that `atomic_deposit_note_v2.masm` assembles cleanly
//! against the same transaction-kernel assembler the v5 controller +
//! production deploy use.
//!
//! Useful as a CI guard whenever the v2 note format or the v5 MAST
//! roots change.

use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{Assembler, DefaultSourceManager, Path};
use miden_protocol::transaction::TransactionKernel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sm: Arc<dyn miden_assembly::SourceManager> =
        Arc::new(DefaultSourceManager::default());

    let math_module = Module::parser(ModuleKind::Library).parse_str(
        Path::new("darwin::math"),
        darwin_protocol_account::MATH_MASM,
        sm.clone(),
    )?;

    let core_lib = miden_core_lib::CoreLibrary::default();
    let math_lib = Assembler::default()
        .with_static_library(core_lib.as_ref())?
        .assemble_library([math_module])?;

    let program = TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(darwin_notes::ATOMIC_DEPOSIT_NOTE_V2_MASM)?;

    println!("✓ atomic_deposit_note_v2 assembles");
    println!("  program hash: {:?}", program.hash());
    Ok(())
}
