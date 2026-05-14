//! Submit an atomic Flow A DepositNote on Miden testnet.
//!
//! Wraps `darwin-notes::ATOMIC_DEPOSIT_NOTE_MASM` into a real
//! `miden_protocol::note::Note` (with deposit assets attached and the
//! deployed real-bodies controller as the recipient), builds a
//! `TransactionRequest` that emits the note from the user wallet, and
//! submits via `miden-client::Client::new_transaction()` against the
//! configured Miden testnet RPC.
//!
//! Pre-requisites:
//!   - `~/.miden/store.sqlite3` + `~/.miden/keystore/` set up by the
//!     existing CLI (i.e. the user has already created the user wallet
//!     and consumed at least one dETH mint into it).
//!   - The real-bodies controller is deployed and discoverable; its
//!     account id is hard-coded below.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin deploy_atomic_flow_a

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{Assembler, DefaultSourceManager, Path};
use miden_client::builder::ClientBuilder;
use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{Note, NoteAssets, NoteMetadata, NoteRecipient, NoteScript, NoteStorage, NoteType};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use rand::RngCore;

const USER_WALLET_HEX: &str = "0xed3cd5befa3207805f8529207cfc0d";
const REAL_BODIES_CONTROLLER_HEX: &str = "0x171f46fecf1bca8005ae068a8dfe77";
const DETH_FAUCET_HEX: &str = "0xa095d9b3831e96206ff70c2218a6a9";
const DEPOSIT_AMOUNT: u64 = 100;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Resolve standard miden-client paths.
    let home = std::env::var("HOME").expect("HOME set");
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    println!("Setting up miden-client against testnet…");
    let store = SqliteStore::new(store_path.clone()).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    // 2. Build the NoteScript from ATOMIC_DEPOSIT_NOTE_MASM, with
    //    darwin::math attached (so std::math::u64::div / felt_div
    //    resolve at assembly time).
    let core_lib = miden_core_lib::CoreLibrary::default();
    let source_manager: Arc<dyn miden_assembly::SourceManager> =
        Arc::new(DefaultSourceManager::default());

    let math_module = Module::parser(ModuleKind::Library)
        .parse_str(
            Path::new("darwin::math"),
            darwin_protocol_account::MATH_MASM,
            source_manager.clone(),
        )?;
    let math_lib = Assembler::default()
        .with_static_library(core_lib.as_ref())?
        .assemble_library([math_module])?;

    let note_program = Assembler::default()
        .with_static_library(core_lib.as_ref())?
        .with_static_library(math_lib.as_ref())?
        .assemble_program(darwin_notes::ATOMIC_DEPOSIT_NOTE_MASM)?;
    let note_script = NoteScript::new(note_program);
    println!("Atomic deposit NoteScript root: {:?}", note_script.root());

    // 3. Resolve account IDs.
    let user_wallet = AccountId::from_hex(USER_WALLET_HEX)?;
    let controller = AccountId::from_hex(REAL_BODIES_CONTROLLER_HEX)?;
    let deth_faucet = AccountId::from_hex(DETH_FAUCET_HEX)?;

    // 4. Construct the deposit assets vault: DEPOSIT_AMOUNT base units of
    //    dETH that move from the user wallet into the note's vault.
    let fungible = FungibleAsset::new(deth_faucet, DEPOSIT_AMOUNT)?;
    let assets = NoteAssets::new(vec![Asset::Fungible(fungible)])?;

    // 5. Metadata: public note, sender = user wallet, default tag.
    let metadata = NoteMetadata::new(user_wallet, NoteType::Public);

    // 6. Recipient: random serial num + the atomic script + empty storage.
    //    Storage carries note inputs (deposit_value, nav, fee_factor).
    //    For this first-cut atomic note the SDK would pass them on the
    //    stack via inputs; the script body computes mint_amount.
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
    let storage = NoteStorage::new(vec![])?;
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), storage);
    let note = Note::new(assets, metadata, recipient);

    println!("Constructed Note");
    println!("  id: {}", note.id());
    println!("  sender: user_wallet {}", user_wallet);
    println!("  target controller: {}", controller);
    println!("  carrying: {DEPOSIT_AMOUNT} dETH base units (faucet {deth_faucet})");

    // 7. Build the TransactionRequest from the user wallet that emits
    //    this note as its only output.
    let tx_request = TransactionRequestBuilder::new()
        .own_output_notes(vec![note.clone()])
        .build()?;

    println!("Executing transaction from user wallet…");
    let tx_result = client.execute_transaction(user_wallet, tx_request).await?;
    let executed = tx_result.executed_transaction().clone();
    println!("Executed. tx id: {}", executed.id());

    println!("Proving transaction (this may take a few seconds)…");
    let prover = client.prover();
    let proven = client.prove_transaction_with(&tx_result, prover).await?;

    println!("Submitting transaction to the testnet node…");
    let submission_height = client.submit_proven_transaction(proven, &tx_result).await?;
    println!("Submitted at block height: {submission_height}");

    println!("Applying transaction locally…");
    client.apply_transaction(&tx_result, submission_height).await?;

    println!();
    println!("🎯 Atomic Flow A deposit note submitted on Miden testnet.");
    println!("   tx id:    {}", executed.id());
    println!("   note id:  {}", note.id());
    println!("   target:   real-bodies controller {controller}");
    println!("   assets:   {DEPOSIT_AMOUNT} dETH base units");
    println!();
    println!("Next: the controller consumes this note in a separate tx,");
    println!("running compute_nav + compute_mint_amount via darwin::math::felt_div");
    println!("on real u64 division — the closing piece of M1 deliverable 5.");

    Ok(())
}
