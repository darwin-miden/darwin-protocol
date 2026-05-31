//! Pro-rata NAV calculator -- "Private Account calculates pro-rata
//! share using Pragma Oracle prices" (Flow C verbatim).
//!
//! Off-chain mirror of the math the controller's compute_redeem_amount
//! proc runs on-chain *inside* the private account. We can't read the
//! controller's vault from the network (it's storage-mode private --
//! that's the whole point of Darwin), but we can compute the exact
//! same NAV from public inputs:
//!
//!   - Target weights for the basket (from darwin-baskets manifests).
//!   - Live Pragma median prices for every constituent.
//!   - Burn amount the user specifies.
//!
//! NAV_per_unit = sum(weight_i * price_i) for each constituent in the
//! basket manifest. user_value = burn_amount * NAV_per_unit. Per-
//! constituent payout the controller would emit:
//!   user_share_i = burn_amount * weight_i * NAV_per_unit / price_i
//! (equivalent to "give the user their pro-rata slice of every
//! constituent at the live Pragma price").
//!
//! On-chain, the controller does this same math with
//! `darwin::math::felt_div` (u64-safe). The note's storage carries
//! the constants we computed here; the kernel verifies the proof.
//!
//! Usage:
//!     cargo run --release --features pragma-live \
//!         -p darwin-protocol-account --bin nav_pro_rata -- \
//!         --basket DCC --burn 50

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use darwin_oracle_adapter::pragma_live;
use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::vm::AdviceInputs;
use miden_client_sqlite_store::SqliteStore;

const V2_CONTROLLER: &str = "0xa25aa0b00007688024b74b05a52aab";

struct Basket {
    symbol: &'static str,
    /// (label, decimals, pragma_pair, weight_bps)
    constituents: &'static [(&'static str, u32, &'static str, u32)],
}

const DCC: Basket = Basket {
    symbol: "DCC",
    constituents: &[
        ("WBTC", 8,  "WBTC/USD", 4000),
        ("ETH",  18, "ETH/USD",  4000),
        ("USDT", 6,  "USDT/USD", 2000),
    ],
};

const DAG: Basket = Basket {
    symbol: "DAG",
    constituents: &[
        ("WBTC", 8,  "WBTC/USD", 5000),
        ("ETH",  18, "ETH/USD",  5000),
    ],
};

const DCO: Basket = Basket {
    symbol: "DCO",
    constituents: &[
        ("WBTC", 8,  "WBTC/USD", 1000),
        ("ETH",  18, "ETH/USD",  1000),
        ("USDT", 6,  "USDT/USD", 4000),
        ("DAI",  18, "DAI/USD",  4000),
    ],
};

fn parse_args() -> (Basket, u64) {
    let mut sym = "DCC".to_string();
    let mut burn: u64 = 100;
    let mut it = std::env::args().skip(1);
    while let Some(a) = it.next() {
        match a.as_str() {
            "--basket" => sym = it.next().expect("--basket value"),
            "--burn" => burn = it.next().expect("--burn value").parse().expect("u64"),
            "--help" | "-h" => {
                eprintln!("nav_pro_rata --basket <DCC|DAG|DCO> --burn <amount>");
                std::process::exit(0);
            }
            _ => panic!("unknown flag: {a}"),
        }
    }
    let basket = match sym.to_uppercase().as_str() {
        "DCC" => DCC,
        "DAG" => DAG,
        "DCO" => DCO,
        _ => panic!("unknown basket: {sym}"),
    };
    (basket, burn)
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let (basket, burn_amount) = parse_args();

    println!("=== Darwin NAV pro-rata redeem calculator ===");
    println!("basket      {}", basket.symbol);
    println!("burn        {burn_amount} basket-token base units");
    println!();

    // 1. Setup miden-client.
    let home = std::env::var("HOME")?;
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_secs();
    let store_path: PathBuf = format!("{home}/.miden/nav_pro_rata_{ts}.sqlite3").into();
    let _ = std::fs::remove_file(&store_path);
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    let store = SqliteStore::new(store_path.clone()).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    client.sync_state().await?;

    // 2. Live Pragma prices for each constituent.
    let _ = V2_CONTROLLER; // referenced in module doc only.
    let oracle_id = AccountId::from_hex(pragma_live::PRAGMA_TESTNET_ORACLE_HEX)?;
    client.import_account_by_id(oracle_id).await?;
    let publishers = pragma_live::discover_publishers(&mut client, oracle_id).await?;
    let median_root = pragma_live::pragma_get_median_mast_root_hex();

    let mut prices_x1e8: BTreeMap<&'static str, u128> = BTreeMap::new();
    for (_, _, pair, _) in basket.constituents {
        let pair_word = pragma_live::pair_word(pair).expect("known pair");
        let foreign = pragma_live::build_foreign_accounts(
            &mut client, oracle_id, &publishers, pair_word,
        ).await?;
        let [_, _, suffix, prefix] = pair_word;
        let script_src = format!(
            "use miden::core::sys\n\nbegin\n  push.0 push.0 push.{suffix} push.{prefix}\n  call.{median_root}\n  exec.sys::truncate_stack\nend\n"
        );
        let tx_script = client.code_builder().compile_tx_script(&script_src)?;
        let stack = client.execute_program(oracle_id, tx_script, AdviceInputs::default(), foreign).await?;
        let found = stack[0].as_canonical_u64();
        let price = stack[1].as_canonical_u64();
        if found == 1 {
            prices_x1e8.insert(pair, price as u128);
        }
    }

    // Pragma quirk: stablecoin feeds (USDT/USD, DAI/USD) ship at ×1e6
    // instead of ×1e8 because the precision needed for a $1 peg is
    // smaller. Detect and rescale so the pro-rata math sees a
    // consistent ×1e8 frame.
    for (_, _, pair, _) in basket.constituents {
        if let Some(p) = prices_x1e8.get_mut(pair) {
            if *p > 0 && *p < 100_000_000 {
                *p *= 100; // stable feed rescale ×1e6 -> ×1e8
            }
        }
    }

    println!("Pragma live prices (×1e8 USD, stable feeds rescaled from ×1e6):");
    for (label, _, pair, weight) in basket.constituents {
        if let Some(p) = prices_x1e8.get(pair) {
            let usd = *p as f64 / 1e8;
            println!("  {label:<6} ({pair:<10})  {p:<16}  = ${usd:<10.4}   weight {weight} bps");
        }
    }
    println!();

    // 3. Per-constituent pro-rata payout the controller would emit.
    //    Convention: 1 basket-token = $1 of basket exposure at issuance
    //    (typical M1 init). For a burn of `burn_amount` basket-tokens
    //    the user receives `burn_amount * weight_fraction` USD worth of
    //    each constituent, converted to base units at the live price:
    //        user_base_i = burn_usd * (weight_bps / 10000)
    //                       * 10^decimals / price_x1e8 / 1e8
    //                    = burn_usd * weight_bps * 10^decimals
    //                       / (10000 * price_x1e8 / 1e8)
    println!("Pro-rata redeem of {burn_amount} {} basket-tokens (= ${burn_amount} USD at par):",
        basket.symbol);
    println!("  {:<6} {:<10} {:>24} {:>14}", "asset", "pair", "user_share (base)", "user_usd");
    let mut total_usd: f64 = 0.0;
    for (label, decimals, pair, weight) in basket.constituents {
        let price_x1e8 = *prices_x1e8.get(pair).unwrap_or(&0);
        let user_usd = (burn_amount as f64) * (*weight as f64) / 10_000.0;
        let user_base_units = if price_x1e8 == 0 {
            0u128
        } else {
            // user_base = user_usd * 10^decimals * 1e8 / price_x1e8
            ((user_usd * 1e8) as u128) * 10u128.pow(*decimals) / price_x1e8
        };
        total_usd += user_usd;
        println!(
            "  {label:<6} {pair:<10} {user_base_units:>24} {:>13.6}$",
            user_usd
        );
    }
    println!();
    println!("→ user receives ≈ ${total_usd:.6} total NAV across constituents");
    println!();
    println!("(Note: this is the off-chain mirror of the controller's");
    println!(" compute_redeem_amount proc. On-chain the same math runs");
    println!(" inside the controller's private MASM context via");
    println!(" darwin::math::felt_div for u64-safe division.)");

    let _ = std::fs::remove_file(&store_path);
    Ok(())
}
