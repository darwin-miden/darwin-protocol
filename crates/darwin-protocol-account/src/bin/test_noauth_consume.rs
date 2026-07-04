//! Test if a fresh client (no keystore) can submit a consume tx
//! against v8-noauth. Success = the "trustless-from-browser" pattern
//! works: anyone can push tx bundles into a NoAuth account without
//! holding any signing key.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;

const V8_NOAUTH: &str = "0x2cc265c53378fb3171eaf12e03c644";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path = PathBuf::from(format!("{home}/.miden/store.sqlite3"));
    let keystore_path = PathBuf::from(format!("{home}/.miden/keystore"));
    std::fs::create_dir_all(&keystore_path)?;

    println!("Connecting to Miden testnet…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    let v8 = AccountId::from_hex(V8_NOAUTH)?;
    println!("Attempting to fetch v8-noauth ({v8})…");
    // If v8 not yet on-chain, import will fail. We'll then try
    // executing a nop tx against it — for NoAuth this SHOULD commit
    // the account's initial state without any signing.
    match client.import_account_by_id(v8).await {
        Ok(_) => println!("v8 already committed on-chain — proceed"),
        Err(e) => {
            println!("v8 not yet committed: {e:?}");
            println!("→ trying an empty tx to commit initial state");
        }
    }
    client.sync_state().await?;

    println!();
    println!("=== execute_transaction(v8, empty request) — no key in keystore ===");
    let req = TransactionRequestBuilder::new().build()?;
    let res = client.execute_transaction(v8, req).await;
    match res {
        Ok(r) => {
            let tx_id = r.executed_transaction().id();
            println!("executed OK, tx_id={tx_id}");
            let prover = client.prover();
            let proven = client.prove_transaction_with(&r, prover).await?;
            let height = client.submit_proven_transaction(proven, &r).await?;
            client.apply_transaction(&r, height).await?;
            println!("submitted at block {height}");
        }
        Err(e) => {
            println!("execute_transaction failed: {e:?}");
        }
    }

    Ok(())
}
