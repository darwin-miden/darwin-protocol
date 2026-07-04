//! Compute + print NoteScript roots for our notes.
//! Used to populate the AuthNetworkAccount allowlist at v8 deploy time.

use std::sync::Arc;
use miden_assembly::{DefaultSourceManager, Path};
use miden_assembly::ast::{Module, ModuleKind};
use miden_protocol::note::NoteScript;
use miden_protocol::transaction::TransactionKernel;

const MATH_NAMESPACE: &str = "darwin::math";
const NOTE_V3: &str = include_str!("../../../darwin-notes/asm/atomic_deposit_note_v3.masm");

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sm: Arc<dyn miden_assembly::SourceManager> = Arc::new(DefaultSourceManager::default());
    let math_mod = Module::parser(ModuleKind::Library)
        .parse_str(Path::new(MATH_NAMESPACE), darwin_protocol_account::MATH_MASM, sm.clone())?;
    let math_lib = TransactionKernel::assembler().assemble_library([math_mod])?;

    let program = TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(NOTE_V3)?;
    let script = NoteScript::new(program);
    println!("atomic_deposit_note_v3 NoteScript root: {}", script.root());
    Ok(())
}
