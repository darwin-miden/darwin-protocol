//! Consume all Committed input notes targeting the local operator
//! wallet, so the faucet drip mBND lands in the account vault.
//!
//! Run AFTER `devnet_sync` reports `Consumable input notes (Committed)`.
//!
//! Usage:
//!     MIDEN_NETWORK=devnet \
//!     HOME=/tmp/miden-devnet-home \
//!     cargo run --release -p darwin-protocol-account --bin devnet_consume_drips

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::Note;
use miden_client::store::NoteFilter;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;

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

    println!("Syncing state…");
    let summary = client.sync_state().await?;
    println!("Synced to block {}", summary.block_num);

    let accounts = client.get_account_headers().await?;
    if accounts.is_empty() {
        return Err("no tracked accounts in this store — create one first".into());
    }
    // Consumer selection: prefer --consumer / DARWIN_CONSUMER_HEX env
    // var; fall back to the first tracked account ONLY when the
    // store has exactly one (single-wallet bootstrap case).
    let consumer_arg = std::env::args()
        .skip(1)
        .scan(false, |take_next, a| {
            if *take_next {
                *take_next = false;
                Some(Some(a))
            } else if a == "--consumer" || a == "-c" {
                *take_next = true;
                Some(None)
            } else {
                Some(None)
            }
        })
        .flatten()
        .next()
        .or_else(|| std::env::var("DARWIN_CONSUMER_HEX").ok());
    let consumer_id = match consumer_arg {
        Some(hex) => AccountId::from_hex(hex.trim())?,
        None => {
            if accounts.len() != 1 {
                return Err(format!(
                    "{} accounts tracked — pass --consumer <hex> or set DARWIN_CONSUMER_HEX",
                    accounts.len()
                )
                .into());
            }
            accounts.into_iter().next().unwrap().0.id()
        }
    };
    println!("Consumer wallet: {}", consumer_id.to_hex());

    let records = client.get_input_notes(NoteFilter::Committed).await?;
    if records.is_empty() {
        println!("No Committed notes ready to consume — try again in a few blocks.");
        return Ok(());
    }
    let mut notes: Vec<Note> = Vec::new();
    for r in records {
        if let Some(id) = r.id() {
            println!("  + including {id}");
        }
        let note: Note = r.try_into()?;
        notes.push(note);
    }

    let tx_request = TransactionRequestBuilder::new().build_consume_notes(notes)?;

    println!("Submitting consume transaction (execute + prove + submit + apply)…");
    let tx_id = client
        .submit_new_transaction(consumer_id, tx_request)
        .await?;
    println!("Submitted. tx id: {tx_id}");

    println!();
    println!("✅ Notes consumed. Run devnet_sync again to confirm the vault balance.");
    Ok(())
}
