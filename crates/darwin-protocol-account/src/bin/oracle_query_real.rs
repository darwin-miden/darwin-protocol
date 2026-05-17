//! Call into the **real** Pragma oracle on Miden testnet from a
//! Darwin transaction script.
//!
//! Closes the M1 deliverable #3 gap by proving Darwin can read a live
//! Pragma price end-to-end on testnet — not just from a mock. Workflow:
//!
//!   1. Compute Pragma's `oracle::get_median` MAST root locally by
//!      re-running their build pipeline (`darwin_oracle_adapter::
//!      pragma_live::pragma_get_median_mast_root_hex`).
//!   2. Discover the publisher account(s) Pragma's oracle aggregates
//!      over (snapshot for the M1 demo).
//!   3. Register both the oracle and every publisher as foreign
//!      accounts, with the right `StorageMapKey` for the queried
//!      pair word (so the kernel can resolve the cross-account
//!      `publisher::get_entry` call inside `get_median`).
//!   4. Submit a tx script that does `call.<get_median_root>` against
//!      the live Pragma oracle and prints the resulting top-of-stack
//!      median price.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --features pragma-live \
//!         --bin oracle_query_real -- --pair ETH/USD

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::vm::AdviceInputs;
use miden_client_sqlite_store::SqliteStore;
use darwin_oracle_adapter::pragma_live;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let pair = args
        .windows(2)
        .find(|w| w[0] == "--pair")
        .map(|w| w[1].clone())
        .unwrap_or_else(|| "ETH/USD".to_string());

    let pair_word = pragma_live::pair_word(&pair).ok_or_else(|| {
        anyhow::anyhow!(
            "unknown pair {pair:?}; supported: BTC/USD, ETH/USD, WBTC/USD, USDT/USD, DAI/USD"
        )
    })?;

    let median_root = pragma_live::pragma_get_median_mast_root_hex();
    println!("pragma::oracle::get_median MAST root (computed locally): {median_root}");
    println!("pair = {pair} → faucet_id_word {pair_word:?}");

    let home = std::env::var("HOME")?;
    // Fresh sqlite path per invocation to dodge stale MMR peaks across runs.
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let store_path: PathBuf = format!("{home}/.miden/oracle_query_real_{ts}.sqlite3").into();
    let _ = std::fs::remove_file(&store_path);
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    println!("Setting up miden-client against testnet…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    let oracle_id = AccountId::from_hex(pragma_live::PRAGMA_TESTNET_ORACLE_HEX)?;

    println!("Syncing state to a recent block…");
    client.sync_state().await?;

    println!("Importing Pragma oracle account…");
    client.import_account_by_id(oracle_id).await?;

    println!("Discovering Pragma publishers…");
    let publishers = pragma_live::discover_publishers(&mut client, oracle_id).await?;
    for p in &publishers {
        println!("  publisher: {p}");
    }

    println!("Building foreign-account map (oracle + {} publishers)…", publishers.len());
    let foreign = pragma_live::build_foreign_accounts(
        &mut client,
        oracle_id,
        &publishers,
        pair_word,
    )
    .await?;

    // Build the tx script. Stack convention from Pragma's median CLI:
    //   push.0.{amount}.{suffix}.{prefix} call.get_median
    // → stack on entry: [prefix, suffix, amount, 0]
    let [_, _, suffix, prefix] = pair_word;
    let script_src = format!(
        "use miden::core::sys

begin
    push.0
    push.0          # amount (unused for spot median)
    push.{suffix}
    push.{prefix}
    call.{median_root}
    exec.sys::truncate_stack
end
"
    );
    let tx_script = client.code_builder().compile_tx_script(&script_src)?;

    println!();
    println!("Executing tx script against Pragma oracle ({}) — calling get_median…", oracle_id);
    let output_stack = client
        .execute_program(oracle_id, tx_script, AdviceInputs::default(), foreign)
        .await?;

    println!();
    println!("🎯 Pragma oracle call landed on-chain. Top-of-stack felts:");
    for (i, felt) in output_stack.iter().enumerate().take(8) {
        println!("  [{i:2}] {}", felt.as_canonical_u64());
    }
    println!();
    println!("Per Pragma's get_median ABI:");
    println!("  [0] = found (1 if pair tracked, else 0)");
    println!("  [1] = median price (×1e8)");
    println!("  [2] = unused (amount echo)");
    Ok(())
}
