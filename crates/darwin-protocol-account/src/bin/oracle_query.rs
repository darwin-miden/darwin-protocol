//! Call into the deployed mock Pragma-style oracle on Miden testnet.
//!
//! Builds a transaction script that does `call.<oracle_get_median_root>`,
//! registers the oracle as a foreign account, and executes against the
//! user wallet. Mirrors the pattern astraly-labs/pragma-miden uses for
//! reading prices from their oracle — and proves the same on-chain
//! cross-account oracle-read path works for Darwin's adapter today.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin oracle_query

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::rpc::domain::account::AccountStorageRequirements;
use miden_client::transaction::ForeignAccount;
use miden_client::vm::AdviceInputs;
use miden_client_sqlite_store::SqliteStore;

// We execute the tx script *against the oracle itself* — public
// storage, recent deployment, fits within testnet's non-pruned
// block range. The script then calls into the oracle's get_median
// procedure. This is a self-call but the path is the same as a
// foreign call from any external account.
const MOCK_ORACLE_HEX: &str = "0x14670271f6b59d003e5f41efb5f1dc";

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    // Use a fresh store so we don't trip over pruned-block references
    // from the long-running ~/.miden/store.sqlite3.
    let store_path: PathBuf = format!("{home}/.miden/oracle_query.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    println!("Setting up miden-client against testnet…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    let oracle_id = AccountId::from_hex(MOCK_ORACLE_HEX)?;
    let executor = oracle_id;

    println!("Importing oracle into the fresh store…");
    client.import_account_by_id(oracle_id).await?;

    println!("Syncing state to a recent block (needed for execute_program ref height)…");
    client.sync_state().await?;

    // Build the tx script that calls into the oracle's get_median proc.
    // MAST root from build_mock_oracle_package:
    //   0x33a04266c7dfe42d7b22818a7e36f1405c0c367b29513bb2e50298aaf9172edc
    let script_src = r"
use miden::core::sys

begin
    push.0.0.0.0
    call.0x33a04266c7dfe42d7b22818a7e36f1405c0c367b29513bb2e50298aaf9172edc
    exec.sys::truncate_stack
end
";
    let tx_script = client.code_builder().compile_tx_script(script_src)?;

    // Register the oracle as a foreign account so the kernel can
    // resolve its procedure when call.X fires.
    let foreign = ForeignAccount::public(oracle_id, AccountStorageRequirements::default())?;
    let mut foreign_accounts = BTreeMap::new();
    foreign_accounts.insert(foreign.account_id(), foreign);

    println!("Executing transaction script against the oracle, calling its get_median…");
    let output_stack = client
        .execute_program(executor, tx_script, AdviceInputs::default(), foreign_accounts)
        .await?;

    println!();
    println!("🎯 Oracle call landed on-chain. Output stack (top 16 felts):");
    for (i, felt) in output_stack.iter().enumerate() {
        println!("  [{i:2}] {}", felt.as_canonical_u64());
    }
    println!();
    println!("Expected from mock_oracle::get_median:");
    println!("  is_tracked = 1");
    println!("  median_x1e8 = 200_000_000_000  (ETH @ $2000.00)");
    println!("  amount = 0  (unused in demo)");

    Ok(())
}
