//! Sync the local Miden store against Devnet, then dump every account
//! the client tracks (id, vault balance per faucet, consumable notes).
//!
//! Built to verify that a faucet drip from `faucet.devnet.miden.io`
//! has landed against the operator wallet created by
//! `create_devnet_operator_wallet`.
//!
//! Usage:
//!     MIDEN_NETWORK=devnet \
//!     HOME=/tmp/miden-devnet-home \
//!     cargo run --release -p darwin-protocol-account --bin devnet_sync

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::store::NoteFilter;
use miden_client_sqlite_store::SqliteStore;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    let endpoint = darwin_protocol_account::miden_endpoint();
    println!("Connecting to Miden ({endpoint:?})…");
    let store = SqliteStore::new(store_path.clone()).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&endpoint, None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    let network_id = client.network_id().await?;
    println!("Connected. network_id = {network_id:?}");

    println!("Syncing state…");
    let summary = client.sync_state().await?;
    println!(
        "Sync complete. block={}, new_public={}, new_private={}, committed={}, consumed={}, updated_accounts={}",
        summary.block_num,
        summary.new_public_notes.len(),
        summary.new_private_notes.len(),
        summary.committed_notes.len(),
        summary.consumed_notes.len(),
        summary.updated_accounts.len(),
    );

    println!();
    println!("── Tracked accounts ──");
    let accounts = client.get_account_headers().await?;
    if accounts.is_empty() {
        println!("  (none)");
    }
    for (header, _seed) in &accounts {
        let id = header.id();
        let bech32 = id.to_bech32(network_id.clone());
        println!("  • {bech32}");
        println!("      hex            : {}", id.to_hex());
        println!("      nonce          : {}", header.nonce());
        if let Some(account) = client.get_account(id).await? {
            let vault = account.vault();
            let asset_count = vault.assets().count();
            println!("      assets in vault: {asset_count}");
            for asset in vault.assets() {
                println!("        - {asset:?}");
            }
        }
    }

    println!();
    println!("── Consumable input notes (Committed) ──");
    let notes = client.get_input_notes(NoteFilter::Committed).await?;
    if notes.is_empty() {
        println!("  (none)");
    }
    for n in notes {
        let id_str = n
            .id()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "<no-id>".into());
        println!("  • note_id={} state={:?}", id_str, n.state());
        if let Some(metadata) = n.metadata() {
            println!("      sender={}", metadata.sender());
        }
    }

    println!();
    println!("── Expected input notes (Expected) ──");
    let expected = client.get_input_notes(NoteFilter::Expected).await?;
    if expected.is_empty() {
        println!("  (none)");
    }
    for n in expected {
        let id_str = n
            .id()
            .map(|id| id.to_string())
            .unwrap_or_else(|| "<no-id>".into());
        println!("  • note_id={} state={:?}", id_str, n.state());
    }

    Ok(())
}
