//! Mint a fungible asset from a faucet to the operator wallet,
//! delivered via a Public P2ID note. Used to seed the operator
//! wallet with dETH (etc.) before the atomic Flow A bringup on
//! Miden Devnet.
//!
//! Usage:
//!     devnet_mint_to_operator <FAUCET_HEX> <RECIPIENT_HEX> <AMOUNT>
//!
//! After running, call `devnet_consume_drips` so the P2ID note
//! lands in the recipient's vault.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::NoteType;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::asset::FungibleAsset;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(format!(
            "usage: {} <FAUCET_HEX> <RECIPIENT_HEX> <AMOUNT>",
            args[0]
        )
        .into());
    }
    let faucet = AccountId::from_hex(&args[1])?;
    let recipient = AccountId::from_hex(&args[2])?;
    let amount: u64 = args[3].parse()?;

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
    client.sync_state().await?;

    // The faucet account itself signs the mint transaction. The
    // recipient gets a Public P2ID note that they consume in a
    // separate tx (devnet_consume_drips).
    let asset = FungibleAsset::new(faucet, amount)
        .map_err(|e| format!("FungibleAsset::new({faucet}, {amount}): {e:?}"))?;

    let tx_request = TransactionRequestBuilder::new().build_mint_fungible_asset(
        asset,
        recipient,
        NoteType::Public,
        client.rng(),
    )?;

    println!(
        "Submitting mint tx from faucet {} → {} ({} base units)…",
        faucet.to_hex(),
        recipient.to_hex(),
        amount
    );
    let tx_id = client.submit_new_transaction(faucet, tx_request).await?;
    println!("Submitted. tx id: {tx_id}");

    println!();
    println!("✅ Mint tx live. Next: run devnet_consume_drips to land the asset in the recipient vault.");
    Ok(())
}
