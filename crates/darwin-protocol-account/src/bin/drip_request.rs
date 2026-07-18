//! Emit a permissionless drip request: a network-tagged note (drip script, no
//! asset) whose storage carries the requester's P2ID payout recipient. The NTX
//! builder executes it against the dispenser, which pays out a fixed 5 dUSDC to
//! the requester from its own vault. This is what a frontend would emit from a
//! user's MidenFi wallet — here driven from the CLI to prove the flow.
//!
//! Env:  HOME=/Users/eden/data/darwin/.v015-asset-faucets  MIDEN_NETWORK=testnet
//! Run:  drip_request <requester_hex> <dispenser_hex>

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{DefaultSourceManager, Path as AsmPath};
use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{
    Note, NoteAssets, NoteRecipient, NoteScript, NoteStorage, NoteTag, NoteType,
    PartialNoteMetadata,
};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::note::{NoteAttachment, NoteAttachments};
use miden_protocol::transaction::TransactionKernel;
use miden_standards::note::{NetworkAccountTarget, NoteExecutionHint, P2idNoteStorage};
use rand::RngCore;

const DUSDC_FAUCET_HEX: &str = "0xfc90f0f4da30e51168453b60eafed7";
const DRIP_AMOUNT: u64 = 5_000_000;

fn rand_word() -> Result<miden_client::Word, Box<dyn std::error::Error>> {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    Ok(miden_client::Word::try_from(
        bytes
            .chunks_exact(8)
            .map(|c| {
                let mut b = [0u8; 8];
                b.copy_from_slice(c);
                miden_client::Felt::new(u64::from_le_bytes(b) & 0xFFFF_FFFE_FFFF_FFFF)
                    .expect("goldilocks")
            })
            .collect::<Vec<_>>()
            .as_slice(),
    )?)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err(format!("usage: {} <requester_hex> <dispenser_hex>", args[0]).into());
    }
    let requester = AccountId::from_hex(&args[1])?;
    let dispenser = AccountId::from_hex(&args[2])?;

    // Drip storage the script reads: [target_suffix, target_prefix, SERIAL(4)].
    // The drip creates a PUBLIC P2ID payout to the requester via p2id::new.
    let serial = rand_word()?;
    let mut storage_felts = vec![requester.suffix(), requester.prefix().as_felt()];
    storage_felts.extend_from_slice(serial.as_elements());

    let dusdc = AccountId::from_hex(DUSDC_FAUCET_HEX)?;
    let script = darwin_protocol_account::drip_note_script(
        dusdc.prefix().as_felt().as_canonical_u64(),
        dusdc.suffix().as_canonical_u64(),
        DRIP_AMOUNT,
    )?;
    let drip_recipient = NoteRecipient::new(rand_word()?, script, NoteStorage::new(storage_felts)?);

    // No asset on the request. Network-tagged so the NTX builder runs it.
    let assets = NoteAssets::new(vec![])?;
    let na = NetworkAccountTarget::new(dispenser, NoteExecutionHint::Always)
        .map_err(|e| format!("NetworkAccountTarget: {e:?}"))?;
    let attachments = NoteAttachments::new(vec![NoteAttachment::from(na)])
        .map_err(|e| format!("NoteAttachments: {e:?}"))?;
    let metadata = PartialNoteMetadata::new(requester, NoteType::Public)
        .with_tag(NoteTag::with_account_target(dispenser));

    let note = Note::with_attachments(assets, metadata, drip_recipient, attachments);
    println!("drip request note id: {}", note.id());
    let payout_recipient = P2idNoteStorage::new(requester).into_recipient(serial);
    let payout_id = Note::new(
        NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(dusdc, DRIP_AMOUNT)?)])?,
        PartialNoteMetadata::new(dispenser, NoteType::Public)
            .with_tag(NoteTag::with_account_target(requester)),
        payout_recipient,
    )
    .id();
    println!("PAYOUT_ID={payout_id}");

    let home = std::env::var("HOME")?;
    let base = format!("{home}/.miden");
    let store = SqliteStore::new(PathBuf::from(format!("{base}/store.sqlite3"))).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(PathBuf::from(format!("{base}/keystore")))?
        .build()
        .await?;
    client.sync_state().await?;

    let req = TransactionRequestBuilder::new()
        .own_output_notes(vec![note])
        .build()?;
    let result = client.execute_transaction(requester, req).await?;
    let tx_id = result.executed_transaction().id();
    let prover = client.prover();
    let proven = client.prove_transaction_with(&result, prover.clone()).await?;
    let height = client.submit_proven_transaction(proven, &result).await?;
    client.apply_transaction(&result, height).await?;

    println!("✓ drip request emitted (network-tagged)");
    println!("    requester : {}", args[1]);
    println!("    dispenser : {}", args[2]);
    println!("    emit tx   : {tx_id} (height {height})");
    println!("    → NTX builder will pay out a PUBLIC P2ID note tagged for the requester.");
    Ok(())
}
