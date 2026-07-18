//! Deposit consumer for the DCC controller. Discovers deposit notes tagged for
//! the controller and consumes them with the controller's key (running the
//! atomic deposit note's `receive_and_credit`, crediting each user's slot-10
//! position + moving the asset into the controller vault). Run ONCE to unstick a
//! stuck deposit, or on a loop as the deposit consumer service.
//!
//! Uses the controller's own store + key (HOME-based, same as backup_write):
//!   HOME=/Users/eden/data/darwin/.relay-miden-testnet MIDEN_NETWORK=testnet \
//!   consume_deposits <controller_hex>

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{Note, NoteTag};
use miden_client::store::NoteFilter;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return Err(format!("usage: {} <controller_hex>", args[0]).into());
    }
    let controller = AccountId::from_hex(&args[1])?;

    let home = std::env::var("HOME")?;
    let base = format!("{home}/.miden");
    let store = SqliteStore::new(PathBuf::from(format!("{base}/store.sqlite3"))).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(PathBuf::from(format!("{base}/keystore")))?
        .build()
        .await?;

    // Subscribe to the controller's account-target tag so sync discovers the
    // deposit notes users emit to it, then pull the latest chain state.
    client
        .add_note_tag(NoteTag::with_account_target(controller))
        .await?;
    client.sync_state().await?;

    let committed = client.get_input_notes(NoteFilter::Committed).await?;
    println!("committed notes tracked for controller: {}", committed.len());

    let prover = client.prover();
    let mut consumed = 0usize;
    for rec in committed {
        let note: Note = match rec.try_into() {
            Ok(n) => n,
            Err(_) => continue,
        };
        let note_id = note.id();
        let req = TransactionRequestBuilder::new()
            .input_notes(vec![(note.clone(), None)])
            .build()?;
        match client.execute_transaction(controller, req).await {
            Ok(r) => {
                let tx = r.executed_transaction().id();
                let proven = client.prove_transaction_with(&r, prover.clone()).await?;
                let h = client.submit_proven_transaction(proven, &r).await?;
                client.apply_transaction(&r, h).await?;
                println!("✓ consumed deposit {note_id} → controller tx {tx} @ block {h}");
                consumed += 1;
            }
            Err(e) => println!("· skip {note_id} (not a deposit for this controller): {e:?}"),
        }
    }
    println!("consumed {consumed} deposit note(s) into controller {controller}");
    Ok(())
}
