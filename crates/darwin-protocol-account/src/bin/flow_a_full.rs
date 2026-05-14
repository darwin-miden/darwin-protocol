//! Full Flow A on Miden testnet, end-to-end in one process.
//!
//! Two transactions submitted back-to-back:
//!   1. User wallet → output note carrying 100 dETH for the controller.
//!      The note script is `ATOMIC_DEPOSIT_NOTE_MASM` and embeds
//!      `darwin::math::felt_div` (real u64 division).
//!   2. Real-bodies controller → consumes the note (unauthenticated),
//!      so the dETH moves into the controller's vault and the note's
//!      script runs on-chain.
//!
//! Builds the Note in-memory and passes it directly to both
//! transactions, avoiding any local sqlite sync round-trip.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin flow_a_full

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{Assembler, DefaultSourceManager, Path};
use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{
    Note, NoteAssets, NoteMetadata, NoteRecipient, NoteScript, NoteStorage, NoteType,
};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use rand::RngCore;

const USER_WALLET_HEX: &str = "0xed3cd5befa3207805f8529207cfc0d";
// v2 real-bodies controller: exposes `receive_asset` so the atomic
// note can drain its vault into the controller. v1 was at
// 0x171f46fecf1bca8005ae068a8dfe77 (no receive_asset).
const REAL_BODIES_CONTROLLER_HEX: &str = "0xa25aa0b00007688024b74b05a52aab";
const DETH_FAUCET_HEX: &str = "0xa095d9b3831e96206ff70c2218a6a9";
const DEPOSIT_AMOUNT: u64 = 100;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    println!("Setting up miden-client against testnet…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    // Build the NoteScript with darwin::math attached.
    let core_lib = miden_core_lib::CoreLibrary::default();
    let sm: Arc<dyn miden_assembly::SourceManager> = Arc::new(DefaultSourceManager::default());
    let math_module = Module::parser(ModuleKind::Library).parse_str(
        Path::new("darwin::math"),
        darwin_protocol_account::MATH_MASM,
        sm.clone(),
    )?;
    let math_lib = Assembler::default()
        .with_static_library(core_lib.as_ref())?
        .assemble_library([math_module])?;
    // Use the TransactionKernel assembler so the note's
    // `use miden::protocol::active_note` etc. resolve.
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(darwin_notes::ATOMIC_DEPOSIT_NOTE_MASM)?;
    let note_script = NoteScript::new(program);

    let user_wallet = AccountId::from_hex(USER_WALLET_HEX)?;
    let controller = AccountId::from_hex(REAL_BODIES_CONTROLLER_HEX)?;
    let deth_faucet = AccountId::from_hex(DETH_FAUCET_HEX)?;
    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(
        deth_faucet,
        DEPOSIT_AMOUNT,
    )?)])?;
    let metadata = NoteMetadata::new(user_wallet, NoteType::Public);

    let mut serial_num_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut serial_num_bytes);
    let serial_num = miden_client::Word::try_from(
        serial_num_bytes
            .chunks_exact(8)
            .map(|chunk| {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(chunk);
                miden_client::Felt::new(u64::from_le_bytes(buf))
            })
            .collect::<Vec<_>>()
            .as_slice(),
    )?;
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), NoteStorage::new(vec![])?);
    let note = Note::new(assets, metadata, recipient);
    println!("Constructed Note id: {}", note.id());

    // -- Step 1: user wallet submits a tx that emits this note ----------
    println!();
    println!("=== Step 1: user wallet emits the atomic deposit note ===");
    let deploy_request = TransactionRequestBuilder::new()
        .own_output_notes(vec![note.clone()])
        .build()?;
    let deploy_result = client.execute_transaction(user_wallet, deploy_request).await?;
    let deploy_tx_id = deploy_result.executed_transaction().id();
    println!("Executed user tx: {deploy_tx_id}");
    let prover = client.prover();
    let deploy_proven = client.prove_transaction_with(&deploy_result, prover.clone()).await?;
    let deploy_height = client.submit_proven_transaction(deploy_proven, &deploy_result).await?;
    println!("Submitted at block: {deploy_height}");
    client.apply_transaction(&deploy_result, deploy_height).await?;
    println!("Local store updated.");

    // -- Step 2: controller submits a tx that consumes the note --------
    println!();
    println!("=== Step 2: controller consumes the atomic deposit note ===");
    let consume_request = TransactionRequestBuilder::new()
        .input_notes(vec![(note.clone(), None)])
        .build()?;
    let consume_result = client.execute_transaction(controller, consume_request).await?;
    let consume_tx_id = consume_result.executed_transaction().id();
    println!("Executed controller tx: {consume_tx_id}");
    let consume_proven = client.prove_transaction_with(&consume_result, prover).await?;
    let consume_height = client.submit_proven_transaction(consume_proven, &consume_result).await?;
    println!("Submitted at block: {consume_height}");
    client.apply_transaction(&consume_result, consume_height).await?;

    println!();
    println!("🎯 FLOW A END-TO-END on Miden testnet:");
    println!("   note id:        {}", note.id());
    println!("   user tx id:     {deploy_tx_id} (block {deploy_height})");
    println!("   consumer tx id: {consume_tx_id} (block {consume_height})");
    println!("   100 dETH moved from user wallet → atomic deposit note → controller vault.");
    println!("   darwin::math::felt_div ran on-chain (miden::core::math::u64::div event).");

    Ok(())
}
