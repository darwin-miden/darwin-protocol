//! Query the live Pragma oracle on Miden testnet for all four Darwin
//! constituents and print a JSON blob to stdout. Consumed by the
//! frontend `/api/prices` route to replace the CoinGecko proxy with a
//! real on-chain read (the mainnet plan from the very first M3 D2 doc).
//!
//! Output shape:
//!   {"source":"pragma-miden","fetchedAt":1779…,"eth":2121.6,
//!    "wbtc":78000.1,"usdt":1.0,"dai":1.0,"pairs":{…}}
//!
//! Each price is the median ×1e8 from `pragma::oracle::get_median`,
//! converted to a f64 USD value (divide by 1e8). The script runs
//! against the testnet RPC; total wall time is ~1s per pair, ~4s for
//! the full set (queried sequentially to keep the sqlite store happy).
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --features pragma-live \
//!         --bin pragma_prices_json
//!
//! Optional flag `--pretty` for human-readable output.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::vm::AdviceInputs;
use miden_client_sqlite_store::SqliteStore;
use darwin_oracle_adapter::pragma_live;

const PAIRS: &[(&str, &str)] = &[
    ("ETH/USD",  "eth"),
    ("WBTC/USD", "wbtc"),
    ("USDT/USD", "usdt"),
    ("DAI/USD",  "dai"),
];

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let pretty = std::env::args().any(|a| a == "--pretty");

    let home = std::env::var("HOME")?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let store_path: PathBuf = format!("{home}/.miden/pragma_prices_{ts}.sqlite3").into();
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
    let median_root = pragma_live::pragma_get_median_mast_root_hex();

    client.sync_state().await?;
    client.import_account_by_id(oracle_id).await?;
    let publishers = pragma_live::discover_publishers(&mut client, oracle_id).await?;

    let mut prices: Vec<(String, Option<f64>)> = Vec::with_capacity(PAIRS.len());
    for (pair_label, _) in PAIRS {
        let pair_word = pragma_live::pair_word(pair_label).ok_or_else(|| {
            anyhow::anyhow!("unknown pair {pair_label}")
        })?;

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
        let stack_result = client
            .execute_program(oracle_id, tx_script, AdviceInputs::default(), foreign)
            .await;

        let price = match stack_result {
            Ok(stack) => {
                let found = stack[0].as_canonical_u64();
                if found == 1 {
                    Some(stack[1].as_canonical_u64() as f64 / 1e8)
                } else {
                    None
                }
            }
            Err(_) => None,
        };
        prices.push((pair_label.to_string(), price));
    }

    let _ = std::fs::remove_file(&store_path);

    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis();

    // Pull the 4 prices in PAIRS order back out into named keys.
    let mut eth = 0.0_f64;
    let mut wbtc = 0.0_f64;
    let mut usdt = 0.0_f64;
    let mut dai = 0.0_f64;
    for ((pair, key), (_, value)) in PAIRS.iter().zip(prices.iter()) {
        let v = value.unwrap_or(0.0);
        match *key {
            "eth" => eth = v,
            "wbtc" => wbtc = v,
            "usdt" => usdt = v,
            "dai" => dai = v,
            _ => {}
        }
        if pretty {
            eprintln!("  {pair} = {v}");
        }
    }

    let json = if pretty {
        format!(
            "{{\n  \"source\": \"pragma-miden\",\n  \"fetchedAt\": {now_ms},\n  \"eth\": {eth},\n  \"wbtc\": {wbtc},\n  \"usdt\": {usdt},\n  \"dai\": {dai}\n}}"
        )
    } else {
        format!(
            "{{\"source\":\"pragma-miden\",\"fetchedAt\":{now_ms},\"eth\":{eth},\"wbtc\":{wbtc},\"usdt\":{usdt},\"dai\":{dai}}}"
        )
    };
    println!("{json}");

    Ok(())
}
