//! Initialize the v5 controller's basket-config storage maps.
//!
//! Submits a signed admin tx against the v5 controller account that
//! calls `set_target_weights` + `set_fees` for the three M1 baskets
//! (DCC, DAG, DCO). The tx ALSO commits the v5 account on-chain — for
//! a freshly registered account this is the first outbound tx.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin deploy_v5_init -- \
//!         --controller 0x089a0ec4270e1480794fed7a21e454

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;

const SET_TARGET_WEIGHTS_ROOT: &str =
    "0x3b6ea663bcd30ab0560635bed559441a71c2a98d71627608f036b43a417f9232";
const SET_FEES_ROOT: &str =
    "0x029e0fd26a948b4e1636a26929da78a2b73d12feeeea6b4c5749cb3ca90ae121";

// M1 basket faucet IDs.
const DCC_FAUCET_HEX: &str = "0x2066f2da1f91ba202af5251d39101c";
const DAG_FAUCET_HEX: &str = "0xfb6811fd6399df206d44f62800620d";
const DCO_FAUCET_HEX: &str = "0xbe4efc6729eb3220423b7d6d6a0942";

/// Target weights per basket, packed as `[w0, w1, w2, 0]` in bps.
/// Matches the m1_submission_state memory's basket framing.
fn weights_for(symbol: &str) -> [u64; 4] {
    match symbol {
        "DCC" => [4000, 4000, 2000, 0], // 40 BTC / 40 ETH / 20 USDT — Core Crypto
        "DAG" => [3000, 4000, 3000, 0], // 30 / 40 / 30 — DeFi Aggregator
        "DCO" => [2000, 5000, 3000, 0], // 20 / 50 / 30 — DeFi/Crypto/Other
        _ => panic!("unknown basket symbol: {symbol}"),
    }
}

/// Fees per basket, packed as `[mint_bps, redeem_bps, mgmt_bps, 0]`.
/// Mirrors the Sepolia DarwinStrategy values exercised in M2.
fn fees_for(_symbol: &str) -> [u64; 4] {
    [200, 150, 100, 0]
}

fn parse_args() -> AccountId {
    let mut controller: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--controller" || a == "-c" {
            controller = Some(args.next().expect("--controller value"));
        }
    }
    let hex = controller.expect("--controller is required");
    AccountId::from_hex(&hex).expect("controller must be a valid Miden AccountId hex")
}

/// Pack a 15-byte AccountId into a 4-felt word as [suffix, prefix, 0, 0]
/// — the storage-map key shape the controller's `get_*` and `set_*`
/// procs expect.
fn basket_key_word(faucet_hex: &str) -> [u64; 4] {
    let id = AccountId::from_hex(faucet_hex).expect("faucet hex");
    [
        id.suffix().as_canonical_u64(),
        id.prefix().as_felt().as_canonical_u64(),
        0,
        0,
    ]
}

fn tx_script_src(controller: AccountId) -> String {
    let mut script = String::new();
    script.push_str("use miden::core::sys\n\nbegin\n");

    for (sym, faucet) in [
        ("DCC", DCC_FAUCET_HEX),
        ("DAG", DAG_FAUCET_HEX),
        ("DCO", DCO_FAUCET_HEX),
    ] {
        let key = basket_key_word(faucet);
        let w = weights_for(sym);
        let f = fees_for(sym);

        script.push_str(&format!("  # ----- {sym} target_weights -----\n"));
        // Push weights_word (w3, w2, w1, w0 — so w0 ends up on top).
        for v in w.iter().rev() {
            script.push_str(&format!("  push.{v}\n"));
        }
        // Push basket_key_word (k3, k2, k1, k0).
        for v in key.iter().rev() {
            script.push_str(&format!("  push.{v}\n"));
        }
        script.push_str(&format!("  call.{SET_TARGET_WEIGHTS_ROOT}\n"));
        script.push_str("  dropw\n"); // discard old value

        script.push_str(&format!("  # ----- {sym} fees -----\n"));
        for v in f.iter().rev() {
            script.push_str(&format!("  push.{v}\n"));
        }
        for v in key.iter().rev() {
            script.push_str(&format!("  push.{v}\n"));
        }
        script.push_str(&format!("  call.{SET_FEES_ROOT}\n"));
        script.push_str("  dropw\n");
    }

    script.push_str("  exec.sys::truncate_stack\n");
    script.push_str("end\n");

    let _ = controller; // signature is informational; sigs come from the keystore
    script
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let controller = parse_args();

    let home = std::env::var("HOME")?;
    let store_path = PathBuf::from(format!("{home}/.miden/store.sqlite3"));
    let keystore_path = PathBuf::from(format!("{home}/.miden/keystore"));

    println!("Connecting miden-client (testnet)…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;
    client.sync_state().await?;

    let script_src = tx_script_src(controller);
    println!(
        "Compiled tx-script — {} lines",
        script_src.lines().count()
    );

    let tx_script = client
        .code_builder()
        .compile_tx_script(&script_src)
        .map_err(|e| format!("compile_tx_script: {e}"))?;

    let request = TransactionRequestBuilder::new()
        .custom_script(tx_script)
        .build()
        .map_err(|e| format!("build request: {e}"))?;

    println!("Submitting admin tx against controller {}…", controller.to_hex());
    let result = client.execute_transaction(controller, request).await?;
    let tx_id = result.executed_transaction().id();
    println!("  executed: {tx_id}");

    let prover = client.prover();
    let proven = client.prove_transaction_with(&result, prover).await?;
    let height = client.submit_proven_transaction(proven, &result).await?;
    client.apply_transaction(&result, height).await?;

    println!();
    println!("✓ v5 controller initialized");
    println!("    controller     : {}", controller.to_hex());
    println!("    init tx        : {tx_id}");
    println!("    block          : {height}");
    println!();
    println!("Slots written:");
    println!("  slot 3 (target_weights): DCC, DAG, DCO");
    println!("  slot 4 (fees)          : DCC, DAG, DCO (200/150/100 bps)");

    Ok(())
}
