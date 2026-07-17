//! E2E/speed test for the on-chain encrypted-backup write path. Writes N test
//! chunks (default 24) into the controller's slot-10 map under backup keys,
//! batched, then read back via POST /api/backup-read to measure the parallel
//! read. Set N via BACKUP_N.
//!
//!   MIDEN_NETWORK=testnet HOME=/Users/eden/data/darwin/.relay-miden-testnet \
//!   cargo run -p darwin-protocol-account --bin backup_write_test

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;

const CONTROLLER_HEX: &str = "0x2cc265c53378fb3171eaf12e03c644";
const SET: &str = "0xea652ac9aa1b6ee468da0845b52008ffa4639d112f356534ba608bc00d7b6f5f";
const SUFFIX: &str = "1656799168076934559";
const PREFIX: &str = "1798790573816354081";
const MAGIC: &str = "15720690719117082606";
const META_INDEX: &str = "4294967295";
const PER_TX: usize = 12; // set_user_position calls per tx

// Write value [v0,v1,v2,v3] under key [suffix,prefix,MAGIC,index]. Push value
// reversed (v0 on top) ⇒ stored word = [v0,v1,v2,v3].
fn write_one(v: [&str; 4], index: usize) -> String {
    format!(
        "  push.{} push.{} push.{} push.{}\n  push.{SUFFIX} push.{PREFIX} push.{MAGIC} push.{index}\n  call.{SET}\n  dropw\n",
        v[3], v[2], v[1], v[0]
    )
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let n: usize = std::env::var("BACKUP_N").ok().and_then(|s| s.parse().ok()).unwrap_or(24);
    let home = std::env::var("HOME")?;
    let store = SqliteStore::new(PathBuf::from(format!("{home}/.miden/store.sqlite3"))).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(PathBuf::from(format!("{home}/.miden/keystore")))?
        .build()
        .await?;
    client.sync_state().await?;
    let controller = AccountId::from_hex(CONTROLLER_HEX)?;

    // Build the list of scripts (chunk batches + meta), then submit each.
    let mut scripts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < n {
        let mut src = String::from("use miden::core::sys\n\nbegin\n");
        let nonce = std::env::var("BACKUP_NONCE").unwrap_or_else(|_| "77".into());
        for j in i..(i + PER_TX).min(n) {
            let v0 = (j + 1).to_string();
            src.push_str(&write_one([&v0, &nonce, "0", "0"], j));
        }
        src.push_str("  exec.sys::truncate_stack\nend\n");
        scripts.push(src);
        i += PER_TX;
    }
    let byte_len = (n * 28).to_string();
    let nw = n.to_string();
    let mut meta = String::from("use miden::core::sys\n\nbegin\n");
    meta.push_str(&write_one([&byte_len, &nw, "0", "0"], META_INDEX.parse::<usize>().unwrap()));
    meta.push_str("  exec.sys::truncate_stack\nend\n");
    scripts.push(meta);

    for (k, src) in scripts.iter().enumerate() {
        let tx = client.code_builder().compile_tx_script(src).map_err(|e| format!("compile: {e}"))?;
        let req = TransactionRequestBuilder::new().custom_script(tx).build().map_err(|e| format!("build: {e}"))?;
        let r = client.execute_transaction(controller, req).await?;
        let prover = client.prover();
        let p = client.prove_transaction_with(&r, prover).await?;
        let h = client.submit_proven_transaction(p, &r).await?;
        client.apply_transaction(&r, h).await?;
        println!("  tx {}/{} → block {h}", k + 1, scripts.len());
    }
    println!("✓ {n} chunks + meta written");
    Ok(())
}
