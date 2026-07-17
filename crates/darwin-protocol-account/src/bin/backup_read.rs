//! Fast in-process reader for the on-chain encrypted backup. Replaces the
//! per-chunk `miden-client exec` process spawns (~35 VM runs for a big backup)
//! with ONE process: sync once, load the controller account, then read every
//! chunk from its slot-10 StorageMap with plain in-memory `get_map_item`
//! lookups. Read time is flat (~sync-time) regardless of backup size.
//!
//! Usage: backup_read <controller_hex> <suffix_u64> <prefix_u64>
//! Prints: {"byteLen": N, "words": [["v0","v1","v2","v3"], …]}  (base-10)
//!
//!   MIDEN_NETWORK=testnet HOME=/Users/eden/data/darwin/.relay-miden-testnet \
//!   backup_read 0x2cc265c53378fb3171eaf12e03c644 1656799168076934559 1798790573816354081

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::{Felt, Word};
use miden_client_sqlite_store::SqliteStore;

// Must match src/lib/onchainBackup.ts and backup_write_test.rs.
const MAGIC: u64 = 15720690719117082606; // 0xda2b1cead0c0ffee
const META_INDEX: u64 = 4294967295; // 0xffffffff

fn felt_str(f: &Felt) -> String {
    f.as_canonical_u64().to_string()
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!("usage: backup_read <controller_hex> <suffix> <prefix>");
        std::process::exit(2);
    }
    let controller = AccountId::from_hex(&args[1])?;
    let suffix: u64 = args[2].parse()?;
    let prefix: u64 = args[3].parse()?;

    let home = std::env::var("HOME")?;
    let store = SqliteStore::new(PathBuf::from(format!("{home}/.miden/store.sqlite3"))).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(PathBuf::from(format!("{home}/.miden/keystore")))?
        .build()
        .await?;
    client.sync_state().await?;

    let account = client
        .get_account(controller)
        .await?
        .ok_or("controller account not found in local store")?;
    let storage = account.storage();

    // The user_positions map (where backups live) is slot 10. Multiple map
    // slots exist (pool_positions is slot 2), so target slot_10 by name; fall
    // back to the map slot with the most entries.
    use miden_client::account::StorageSlotContent;
    let map_slot = storage
        .slots()
        .iter()
        .find(|s| s.name().as_str().ends_with("slot_10") && s.slot_type().is_map())
        .or_else(|| {
            storage
                .slots()
                .iter()
                .filter(|s| s.slot_type().is_map())
                .max_by_key(|s| match s.content() {
                    StorageSlotContent::Map(m) => m.num_entries(),
                    _ => 0,
                })
        })
        .ok_or("no user_positions map slot on controller")?;
    let slot_name = map_slot.name().clone();

    if std::env::var("DEBUG").is_ok() {
        use miden_client::account::StorageSlotContent;
        eprintln!("[dbg] slots={}", storage.slots().len());
        for s in storage.slots() {
            match s.content() {
                StorageSlotContent::Map(m) => eprintln!(
                    "[dbg]   {} MAP entries={}",
                    s.name().as_str(),
                    m.num_entries()
                ),
                StorageSlotContent::Value(_) => {
                    eprintln!("[dbg]   {} VALUE", s.name().as_str())
                }
            }
        }
    }

    // Key word matches the MASM push order (index on top ⇒ word[0]=index):
    // [index, MAGIC, prefix, suffix]. Value word reads back as [v0,v1,v2,v3].
    let read = |index: u64| -> Result<[Felt; 4], Box<dyn std::error::Error>> {
        let key = Word::from([
            Felt::new_unchecked(index),
            Felt::new_unchecked(MAGIC),
            Felt::new_unchecked(prefix),
            Felt::new_unchecked(suffix),
        ]);
        let w = storage.get_map_item(&slot_name, key)?;
        let e = w.as_elements();
        Ok([e[0], e[1], e[2], e[3]])
    };

    // Meta entry → [byteLen, nWords, 0, 0].
    let meta = read(META_INDEX)?;
    let byte_len = meta[0].as_canonical_u64();
    let n_words = meta[1].as_canonical_u64();
    if byte_len == 0 || n_words == 0 {
        println!("{{\"byteLen\":0,\"words\":[]}}");
        return Ok(());
    }

    let mut words = String::from("[");
    for i in 0..n_words {
        let v = read(i)?;
        if i > 0 {
            words.push(',');
        }
        words.push_str(&format!(
            "[\"{}\",\"{}\",\"{}\",\"{}\"]",
            felt_str(&v[0]),
            felt_str(&v[1]),
            felt_str(&v[2]),
            felt_str(&v[3])
        ));
    }
    words.push(']');
    println!("{{\"byteLen\":{byte_len},\"words\":{words}}}");
    Ok(())
}
