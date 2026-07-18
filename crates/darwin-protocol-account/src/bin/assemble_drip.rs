//! Assemble the permissionless drip note script and print its NoteScript root.
//! Placeholder felts — assembly doesn't check them; the real values are
//! templated in by deploy_dispenser at deploy time.

use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{DefaultSourceManager, Path};
use miden_protocol::transaction::TransactionKernel;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let sm: Arc<dyn miden_assembly::SourceManager> = Arc::new(DefaultSourceManager::default());

    // Link the BasicWallet library so `wallet::move_asset_to_note` resolves.
    let wallet_module = Module::parser(ModuleKind::Library).parse_str(
        Path::new("miden::standards::wallets::basic"),
        darwin_notes::STD_BASIC_WALLET_MASM,
        sm.clone(),
    )?;
    let wallet_lib = TransactionKernel::assembler().assemble_library([wallet_module])?;

    let src = darwin_notes::DRIP_NOTE_MASM
        .replace("{{DRIP_AMOUNT}}", "5000000")
        .replace("{{DUSDC_FAUCET_PREFIX}}", "0")
        .replace("{{DUSDC_FAUCET_SUFFIX}}", "0");

    let program = TransactionKernel::assembler()
        .with_static_library(wallet_lib.as_ref())?
        .assemble_program(&src)?;

    println!("✓ drip_note assembles");
    println!("  root: {:?}", program.hash());
    Ok(())
}
