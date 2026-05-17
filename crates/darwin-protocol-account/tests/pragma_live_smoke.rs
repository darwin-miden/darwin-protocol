//! E2E smoke test: Darwin → live Pragma oracle on Miden testnet →
//! real median price returned on stack.
//!
//! Skipped by default (`#[ignore]`) because it hits the network. Run
//! explicitly with:
//!
//!   cargo test -p darwin-protocol-account --features pragma-live \
//!     pragma_live_smoke -- --ignored --nocapture
//!
//! Doubles as a self-check for the fallback path: if the live Pragma
//! oracle is unreachable or returns `found=0`, the test fails loudly
//! so the next M1 status report doesn't silently regress.

#![cfg(feature = "pragma-live")]

use std::path::PathBuf;
use std::sync::Arc;

use darwin_oracle_adapter::pragma_live;
use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::vm::AdviceInputs;
use miden_client_sqlite_store::SqliteStore;

#[tokio::test]
#[ignore = "hits Miden testnet RPC + Pragma live oracle; run with --ignored"]
async fn darwin_can_read_eth_usd_from_live_pragma() -> anyhow::Result<()> {
    let pair = "ETH/USD";
    let pair_word = pragma_live::pair_word(pair).expect("pair supported");

    let median_root = pragma_live::pragma_get_median_mast_root_hex();
    let home = std::env::var("HOME")?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let store_path: PathBuf =
        format!("{home}/.miden/pragma_live_smoke_{ts}.sqlite3").into();
    let _ = std::fs::remove_file(&store_path);
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    let store = SqliteStore::new(store_path.clone()).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    let oracle_id = AccountId::from_hex(pragma_live::PRAGMA_TESTNET_ORACLE_HEX)?;

    client.sync_state().await?;
    client.import_account_by_id(oracle_id).await?;

    let publishers = pragma_live::discover_publishers(&mut client, oracle_id).await?;
    assert!(!publishers.is_empty(), "no publishers discovered");

    let foreign = pragma_live::build_foreign_accounts(
        &mut client,
        oracle_id,
        &publishers,
        pair_word,
    )
    .await?;

    let [_, _, suffix, prefix] = pair_word;
    let script_src = format!(
        "use miden::core::sys

begin
    push.0
    push.0
    push.{suffix}
    push.{prefix}
    call.{median_root}
    exec.sys::truncate_stack
end
"
    );
    let tx_script = client.code_builder().compile_tx_script(&script_src)?;
    let stack = client
        .execute_program(oracle_id, tx_script, AdviceInputs::default(), foreign)
        .await?;

    // Pragma's get_median ABI: stack[0] = found flag, stack[1] = median × 1e8.
    let found = stack[0].as_canonical_u64();
    let median = stack[1].as_canonical_u64();
    assert_eq!(found, 1, "Pragma oracle did not return found=1 for {pair}");
    assert!(median > 0, "median price must be positive");
    // Plausibility band for ETH on a 6-month timescale: $200 – $20_000.
    let dollars = median / 100_000_000;
    assert!(
        (200..=20_000).contains(&dollars),
        "ETH median {dollars} USD outside plausibility band [200, 20000]"
    );
    println!("✅ Darwin read live Pragma {pair} = ${}.{:08}", dollars, median % 100_000_000);

    let _ = std::fs::remove_file(&store_path);
    Ok(())
}
