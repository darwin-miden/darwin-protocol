//! Minimal deposit note test — emits + consumes a note whose script
//! is JUST the drain_assets_into_controller loop (no compute_mint
//! step). Isolates whether the v0.15 consume failure is caused by:
//!   - the asset-drain MASM itself (this test would fail), or
//!   - the preceding math/storage step messing up the stack (this
//!     test would pass).
//!
//! Same exit semantics as the v0.15 standard wallet's
//! add_assets_to_account, so if THIS fails on Devnet the bug is at
//! the asset-loading layer, not in our compute step.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{
    Note, NoteAssets, NoteRecipient, NoteScript, NoteStorage, NoteType,
};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use rand::RngCore;

const USER_WALLET_HEX: &str = "0x4397442ac860af717888fe90cad00b";
const CONTROLLER_HEX: &str = "0x2388eaea4ce45331214b871755e7b5";
const DETH_FAUCET_HEX: &str = "0xc2c923560dc3cb114ec24ab2291a05";

// v7 controller's receive_asset MAST root (rotated under v0.15).
const RECEIVE_ASSET_ROOT: &str =
    "0x6170fd6d682d91777b551fd866258f43cc657f1291f8f071500f4e56e9c153da";

const DEPOSIT_AMOUNT: u64 = 50;

/// Minimal MASM: NO storage read, NO math, NO mint_amount compute.
/// Just the asset-drain loop, byte-identical to the v0.15 standard
/// wallet's `add_assets_to_account` proc but calling the v7
/// controller's receive_asset root instead of the in-wallet ref.
fn build_minimal_note_masm() -> String {
    format!(
        r#"
use miden::protocol::active_note
use miden::protocol::asset::ASSET_VALUE_MEMORY_OFFSET
use miden::protocol::asset::ASSET_SIZE

@locals(2048)
proc drain_only
    locaddr.0 exec.active_note::get_assets
    # standard wallet's pattern: re-emit base ptr then build end_ptr.
    mul.ASSET_SIZE locaddr.0 dup movdn.2 add
    # => [end_ptr, ptr]
    padw padw movup.9
    dup dup.10 neq
    while.true
        dup movdn.9
        add.ASSET_VALUE_MEMORY_OFFSET mem_loadw_le swapw
        dup.8 mem_loadw_le
        padw padw swapdw
        call.{RECEIVE_ASSET_ROOT}
        dropw dropw
        movup.8 add.ASSET_SIZE dup
        dup.10 neq
    end
    drop dropw dropw drop
end

begin
    exec.drain_only
end
"#
    )
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    let endpoint = darwin_protocol_account::miden_endpoint();
    println!("Connecting to Miden ({endpoint:?})…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&endpoint, None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;
    client.sync_state().await?;

    let masm = build_minimal_note_masm();
    println!("Minimal MASM:\n{masm}");
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .assemble_program(masm.as_str())?;
    let note_script = NoteScript::new(program);

    let user_wallet = AccountId::from_hex(USER_WALLET_HEX)?;
    let controller = AccountId::from_hex(CONTROLLER_HEX)?;
    let deth_faucet = AccountId::from_hex(DETH_FAUCET_HEX)?;
    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(
        deth_faucet,
        DEPOSIT_AMOUNT,
    )?)])?;
    let metadata =
        miden_protocol::note::PartialNoteMetadata::new(user_wallet, NoteType::Public);

    let mut serial = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut serial);
    let serial_num = miden_client::Word::try_from(
        serial
            .chunks_exact(8)
            .map(|c| {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(c);
                miden_client::Felt::new(u64::from_le_bytes(buf) & 0xFFFF_FFFE_FFFF_FFFF)
                    .expect("masked")
            })
            .collect::<Vec<_>>()
            .as_slice(),
    )?;
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), NoteStorage::new(vec![])?);
    let note = Note::new(assets, metadata, recipient);
    println!("Note id: {}", note.id());

    println!("=== Step 1: emit ===");
    let deploy = TransactionRequestBuilder::new()
        .own_output_notes(vec![note.clone()])
        .build()?;
    let r = client.execute_transaction(user_wallet, deploy).await?;
    let prover = client.prover();
    let p = client.prove_transaction_with(&r, prover.clone()).await?;
    let h = client.submit_proven_transaction(p, &r).await?;
    client.apply_transaction(&r, h).await?;
    println!("Emitted at block {h}");

    println!("=== Step 2: controller consume ===");
    let consume = TransactionRequestBuilder::new()
        .input_notes(vec![(note.clone(), None)])
        .build()?;
    let r = client.execute_transaction(controller, consume).await?;
    let p = client.prove_transaction_with(&r, prover).await?;
    let h = client.submit_proven_transaction(p, &r).await?;
    client.apply_transaction(&r, h).await?;
    println!("✅ Controller consume succeeded at block {h} — bug is in compute_mint step.");

    Ok(())
}
