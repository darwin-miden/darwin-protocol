//! Read slot-10 positions from the v9 network controller via the local
//! store (sync first so NTB-driven writes are visible).
//!
//!     cargo run -p darwin-protocol-account --bin read_v9_position -- \
//!         --account 0xded5aaaedbd1d55163ac0480838229

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::account::{StorageSlotContent, StorageSlotName};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut account_hex: Option<String> = None;
    let mut json = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--account" => account_hex = Some(args.next().expect("--account value")),
            "--json" => json = true,
            _ => {}
        }
    }
    let account_id = AccountId::from_hex(&account_hex.expect("--account required"))?;

    let home = std::env::var("HOME")?;
    let base = std::env::var("MIDEN_STORE_DIR")
        .unwrap_or_else(|_| format!("{home}/.miden-v9-network"));
    let store = SqliteStore::new(PathBuf::from(format!("{base}/store.sqlite3"))).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(PathBuf::from(format!("{base}/keystore")))?
        .build()
        .await?;
    client.sync_state().await?;

    let account = client
        .get_account(account_id)
        .await?
        .ok_or("account not tracked in this store")?;

    if json {
        // Machine output for the /api/network-position route: one line of
        // JSON with every slot-10 map entry as {key, amount}.
        let slot10 = StorageSlotName::new("darwin::slot_10".to_string())?;
        let mut entries = Vec::new();
        for slot in account.storage().slots() {
            if slot.name() != &slot10 {
                continue;
            }
            if let StorageSlotContent::Map(map) = slot.content() {
                for (key, value) in map.entries() {
                    let amount: u128 = value
                        .as_elements()
                        .iter()
                        .map(|f| f.as_canonical_u64() as u128)
                        .sum();
                    entries.push(format!(
                        "{{\"key\":\"{key}\",\"amount\":\"{amount}\"}}"
                    ));
                }
            }
        }
        println!("{{\"entries\":[{}]}}", entries.join(","));
        return Ok(());
    }

    println!("account : {}", account.id().to_hex());
    println!("nonce   : {}", account.nonce());
    println!("vault   :");
    for asset in account.vault().assets() {
        println!("    {asset:?}");
    }

    let slot10 = StorageSlotName::new("darwin::slot_10".to_string())?;
    println!();
    println!("slot-10 user_positions map entries:");
    for slot in account.storage().slots() {
        if slot.name() != &slot10 {
            continue;
        }
        match slot.content() {
            StorageSlotContent::Map(map) => {
                let mut n = 0;
                for (key, value) in map.entries() {
                    println!("    key   = {key}");
                    println!("    value = {value}");
                    n += 1;
                }
                if n == 0 {
                    println!("    (empty)");
                }
            }
            _ => println!("    (not a map?)"),
        }
    }

    Ok(())
}
