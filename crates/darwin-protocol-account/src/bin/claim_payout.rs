//! Claim a drip payout: read the private payout NoteFile that drip_request
//! wrote, reconstruct the note, and consume it against the requester. This is
//! the requester-side of the permissionless drip (the frontend does the
//! equivalent importNoteFile + consume).
//!
//! Run: claim_payout <requester_hex> <dispenser_hex> <payout_file>

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{Note, NoteType, PartialNoteMetadata};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::note::NoteFile;
use miden_protocol::utils::serde::Deserializable;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(format!("usage: {} <requester_hex> <dispenser_hex> <payout_file>", args[0]).into());
    }
    let requester = AccountId::from_hex(&args[1])?;
    let dispenser = AccountId::from_hex(&args[2])?;

    let bytes = std::fs::read(&args[3])?;
    let nf = NoteFile::read_from_bytes(&bytes).map_err(|e| format!("read NoteFile: {e:?}"))?;
    let details = match nf {
        NoteFile::NoteDetails { details, .. } => details,
        _ => return Err("expected NoteFile::NoteDetails".into()),
    };
    // Rebuild the full note (metadata doesn't affect the nullifier/id — only
    // recipient + assets do, which come from the details).
    let assets = details.assets().clone();
    let recipient = details.recipient().clone();
    let metadata = PartialNoteMetadata::new(dispenser, NoteType::Private);
    let note = Note::new(assets, metadata, recipient);
    println!("payout note id: {}", note.id());

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
        .input_notes(vec![(note, None)])
        .build()?;
    match client.execute_transaction(requester, req).await {
        Ok(result) => {
            let tx_id = result.executed_transaction().id();
            let prover = client.prover();
            let proven = client.prove_transaction_with(&result, prover.clone()).await?;
            let height = client.submit_proven_transaction(proven, &result).await?;
            client.apply_transaction(&result, height).await?;
            println!("✓✓✓ payout claimed — requester received the dUSDC (tx {tx_id}, height {height})");
        }
        Err(e) => println!("✗ claim failed: {e:?}"),
    }
    Ok(())
}
