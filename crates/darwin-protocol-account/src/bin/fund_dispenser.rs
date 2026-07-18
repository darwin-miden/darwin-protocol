//! Fund the permissionless dUSDC dispenser (a network account) by emitting a
//! P2ID note carrying dUSDC that is TAGGED for network execution + carries a
//! NetworkAccountTarget attachment — so the NTX builder actually consumes it
//! into the dispenser's vault. Plain `miden-client send` does NOT do this (no
//! network tag), which is why the earlier funding attempts were never consumed.
//!
//! Env:  HOME=/Users/eden/data/darwin/.v015-asset-faucets  MIDEN_NETWORK=testnet
//! Run:  fund_dispenser <dispenser_hex> <amount_base_units>
//! Sender = the wallet dispenser that holds the bridged dUSDC.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{Note, NoteAssets, NoteTag, NoteType, PartialNoteMetadata};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::note::{NoteAttachment, NoteAttachments};
use miden_standards::note::{NetworkAccountTarget, NoteExecutionHint, P2idNoteStorage};
use rand::RngCore;

const WALLET_DISP: &str = "0xaea2b3093957cc7163f64a64a297c6"; // holds bridged dUSDC
const DUSDC: &str = "0xfc90f0f4da30e51168453b60eafed7";

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
        return Err(format!("usage: {} <dispenser_hex> <amount_base_units>", args[0]).into());
    }
    let dispenser = AccountId::from_hex(&args[1])?;
    let amount: u64 = args[2].parse()?;
    let sender = AccountId::from_hex(WALLET_DISP)?;
    let dusdc = AccountId::from_hex(DUSDC)?;

    // P2ID note targeting the dispenser, carrying dUSDC.
    let serial = rand_word()?;
    let recipient = P2idNoteStorage::new(dispenser).into_recipient(serial);
    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(dusdc, amount)?)])?;

    // Network-execution routing: tag + NetworkAccountTarget so the NTX builder
    // executes this note against the dispenser (which allowlists P2ID).
    let na = NetworkAccountTarget::new(dispenser, NoteExecutionHint::Always)
        .map_err(|e| format!("NetworkAccountTarget: {e:?}"))?;
    let attachments = NoteAttachments::new(vec![NoteAttachment::from(na)])
        .map_err(|e| format!("NoteAttachments: {e:?}"))?;
    let metadata = PartialNoteMetadata::new(sender, NoteType::Public)
        .with_tag(NoteTag::with_account_target(dispenser));

    let note = Note::with_attachments(assets, metadata, recipient, attachments);
    println!("funding note id: {}", note.id());

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
    let result = client.execute_transaction(sender, req).await?;
    let tx_id = result.executed_transaction().id();
    let prover = client.prover();
    let proven = client.prove_transaction_with(&result, prover.clone()).await?;
    let height = client.submit_proven_transaction(proven, &result).await?;
    client.apply_transaction(&result, height).await?;

    println!("✓ funding note emitted (network-tagged)");
    println!("    amount : {amount} dUSDC base units → dispenser {}", args[1]);
    println!("    emit tx: {tx_id} (height {height})");
    println!("The NTX builder should now consume it into the dispenser's vault.");
    Ok(())
}
