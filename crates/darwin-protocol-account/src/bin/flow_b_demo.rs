//! Flow B end-to-end demo on Miden testnet — M2 Track 3.
//!
//! Submits a Flow B *trigger note* (zero assets, calls into the
//! v4 controller's `execute_rebalance_step` proc) from the user
//! wallet, then has the v4 controller consume it. Mirrors the
//! flow_a_full / flow_c_full structure.
//!
//! Pre-requisite: a v4 rebalance-aware controller must be deployed.
//! Build it with:
//!
//!     cargo run -p darwin-protocol-account \
//!         --bin build_v4_rebalance_controller -- \
//!         --out /tmp/darwin-v4-rebalance-controller.masp
//!
//! Deploy with:
//!
//!     miden client new-account \
//!         --account-type regular-account-immutable-code \
//!         --packages /tmp/darwin-v4-rebalance-controller.masp \
//!         --storage-mode private --deploy
//!
//! Then run this binary with the resulting controller id:
//!
//!     cargo run -p darwin-protocol-account --bin flow_b_demo -- \
//!         --controller 0x<v4-controller-hex>

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{
    Note, NoteAssets, NoteMetadata, NoteRecipient, NoteScript, NoteStorage, NoteType,
};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use rand::RngCore;

const USER_WALLET_HEX: &str = "0xed3cd5befa3207805f8529207cfc0d";

fn parse_args() -> String {
    let mut controller: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--controller" || a == "-c" {
            controller = Some(args.next().expect("--controller value"));
        }
    }
    controller.unwrap_or_else(|| {
        eprintln!(
            "ERROR: --controller <hex> required\n  (deploy a v4 controller first; see binary doc)"
        );
        std::process::exit(2)
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let controller_hex = parse_args();

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

    // Build the trigger NoteScript. The script imports miden::core::sys
    // and `call.X`s into the v4 controller's execute_rebalance_step
    // MAST root (hardcoded in the .masm source). No darwin::math
    // library needed — the trigger script is pure compute over the
    // stack.
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .assemble_program(darwin_notes::REBALANCE_TRIGGER_NOTE_MASM)?;
    let note_script = NoteScript::new(program);

    let user_wallet = AccountId::from_hex(USER_WALLET_HEX)?;
    let controller = AccountId::from_hex(&controller_hex)?;

    // Flow B trigger note carries zero assets.
    let assets = NoteAssets::new(vec![])?;
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
    println!("Constructed TriggerNote id: {}", note.id());

    // -- Step 1: user wallet emits the trigger note -------------------
    println!();
    println!("=== Step 1: user wallet emits the Flow B trigger note ===");
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

    // -- Step 2: v4 controller consumes the trigger note --------------
    println!();
    println!("=== Step 2: v4 controller consumes the trigger note ===");
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
    println!("🎯 FLOW B END-TO-END on Miden testnet:");
    println!("   note id:        {}", note.id());
    println!("   user tx id:     {deploy_tx_id} (block {deploy_height})");
    println!("   consumer tx id: {consume_tx_id} (block {consume_height})");
    println!("   v4 controller:  {controller_hex}");
    println!();
    println!("execute_rebalance_step ran on-chain inside the controller's tx context.");
    println!("M2 follow-up: emit per-asset swap notes targeting the mock DEX account.");

    Ok(())
}
