//! Initialize the v6 controller's basket-config storage maps.
//!
//! Submits a signed admin tx against the v6 controller account that
//! calls `set_target_weights` + `set_fees` for the three M1 baskets
//! (DCC, DAG, DCO). The tx ALSO commits the v5 account on-chain — for
//! a freshly registered account this is the first outbound tx.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin deploy_v6_init -- \
//!         --controller 0x089a0ec4270e1480794fed7a21e454

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;

// MAST roots come from `build_v6_fee_routing_controller`. Two
// generations are pinned because the v0.14 (testnet) controller and
// the v0.15 (devnet) controller answer to different procedure roots
// — wire format 0.0.2 → 0.0.3 rotates every root. The active set is
// selected at runtime by `MIDEN_NETWORK`.

const SET_TARGET_WEIGHTS_ROOT_V014: &str =
    "0x57a8ef319a2fe090f649760c4db4fdfc698496778daaea8f496cc46070e4057c";
const SET_FEES_ROOT_V014: &str =
    "0xf2624ee2a579f81446f60cba7fdb06058c36fa2a06fc1b67accaafdd0d86e3f8";
const SET_FEE_RECIPIENT_ROOT_V014: &str =
    "0x6721d6156a7a78b8eea224963e4375ee7423ac2d2f79d58a1c5af542f370d9a4";

const SET_TARGET_WEIGHTS_ROOT_V015: &str =
    "0x26a369cd6781d75d40169223996afcc98602775ce4b6fe9bba8236eb70ceb8e2";
const SET_FEES_ROOT_V015: &str =
    "0xfbcc4fdd3852fc7ea0325e62377af7692845f558a2f0e503c5c52edbbde1ed26";
const SET_FEE_RECIPIENT_ROOT_V015: &str =
    "0xfed8024bd5134e2229d0ac853fd9191bf3f43479e85af96499c4a05785df6e6c";

fn is_devnet() -> bool {
    std::env::var("MIDEN_NETWORK")
        .ok()
        .map(|v| v.eq_ignore_ascii_case("devnet"))
        .unwrap_or(false)
}

/// 2026-06-23: testnet was migrated to v0.15 on Miden's side, so the
/// "v0.14 roots" branch is now only useful for hypothetical localhost
/// deployments. Both `devnet` and `testnet` networks use the v0.15
/// roots; switch to V014 only when explicitly running against an
/// older node.
fn use_v015_roots() -> bool {
    let net = std::env::var("MIDEN_NETWORK").unwrap_or_else(|_| "testnet".into());
    matches!(net.to_ascii_lowercase().as_str(), "devnet" | "testnet")
}

fn set_target_weights_root() -> &'static str {
    if use_v015_roots() {
        SET_TARGET_WEIGHTS_ROOT_V015
    } else {
        SET_TARGET_WEIGHTS_ROOT_V014
    }
}
fn set_fees_root() -> &'static str {
    if use_v015_roots() {
        SET_FEES_ROOT_V015
    } else {
        SET_FEES_ROOT_V014
    }
}
fn set_fee_recipient_root() -> &'static str {
    if use_v015_roots() {
        SET_FEE_RECIPIENT_ROOT_V015
    } else {
        SET_FEE_RECIPIENT_ROOT_V014
    }
}

// Slot 11 (fee_recipient): on testnet this is the relay wallet
// (sweeps L1↔L2 fees back into its vault). On Devnet bootstrap we
// haven't deployed a relay wallet yet, so default to the operator
// wallet that funds the deploy. Override via DARWIN_FEE_RECIPIENT_HEX
// once the relay redeploys.
//
// 2026-06-23: testnet v0.15 redeploy — operator wallet 0xd5638369…
// stands in for the fee recipient until a fresh relay wallet is wired.
const FEE_RECIPIENT_HEX_TESTNET: &str = "0xd563836959ebc61129e70dd5ab4e1a";
const FEE_RECIPIENT_HEX_DEVNET: &str = "0x4397442ac860af717888fe90cad00b";

// Basket-token faucets. Testnet hexes captured during the 2026-06-23
// v0.15 redeploy (deploy_devnet_faucet output). Devnet hexes captured
// during the 2026-06-20 Devnet redeploy.
const DCC_FAUCET_HEX_TESTNET: &str = "0x4eb76287e07e90714a86ae2b89d700";
const DAG_FAUCET_HEX_TESTNET: &str = "0xed4219cb5ebf3d911c27dc6b24baa2";
const DCO_FAUCET_HEX_TESTNET: &str = "0xc58107b160df13d1157b707e3f0a3d";
const DCC_FAUCET_HEX_DEVNET: &str = "0x536e8b33e2e10d915bd466faa64099";
const DAG_FAUCET_HEX_DEVNET: &str = "0x6c4f5da5061c6f312e99327a5b36d3";
const DCO_FAUCET_HEX_DEVNET: &str = "0xf1be7df227291a714c62658a3bcd18";

fn fee_recipient_hex() -> String {
    std::env::var("DARWIN_FEE_RECIPIENT_HEX").unwrap_or_else(|_| {
        if is_devnet() {
            FEE_RECIPIENT_HEX_DEVNET.into()
        } else {
            FEE_RECIPIENT_HEX_TESTNET.into()
        }
    })
}
fn dcc_faucet_hex() -> &'static str {
    if is_devnet() {
        DCC_FAUCET_HEX_DEVNET
    } else {
        DCC_FAUCET_HEX_TESTNET
    }
}
fn dag_faucet_hex() -> &'static str {
    if is_devnet() {
        DAG_FAUCET_HEX_DEVNET
    } else {
        DAG_FAUCET_HEX_TESTNET
    }
}
fn dco_faucet_hex() -> &'static str {
    if is_devnet() {
        DCO_FAUCET_HEX_DEVNET
    } else {
        DCO_FAUCET_HEX_TESTNET
    }
}

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

    let set_target_weights_root = set_target_weights_root();
    let set_fees_root = set_fees_root();
    let set_fee_recipient_root = set_fee_recipient_root();

    // ----- v6: write fee_recipient to slot 11 -----
    let fee_recipient_hex = fee_recipient_hex();
    let fee_recipient_id =
        AccountId::from_hex(&fee_recipient_hex).expect("fee recipient hex");
    let fee_value = [
        fee_recipient_id.suffix().as_canonical_u64(),
        fee_recipient_id.prefix().as_felt().as_canonical_u64(),
        0,
        0,
    ];
    script.push_str("  # ----- fee_recipient (slot 11) -----\n");
    for v in fee_value.iter().rev() {
        script.push_str(&format!("  push.{v}\n"));
    }
    script.push_str(&format!("  call.{set_fee_recipient_root}\n"));
    script.push_str("  dropw\n");

    for (sym, faucet) in [
        ("DCC", dcc_faucet_hex()),
        ("DAG", dag_faucet_hex()),
        ("DCO", dco_faucet_hex()),
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
        script.push_str(&format!("  call.{set_target_weights_root}\n"));
        script.push_str("  dropw\n"); // discard old value

        script.push_str(&format!("  # ----- {sym} fees -----\n"));
        for v in f.iter().rev() {
            script.push_str(&format!("  push.{v}\n"));
        }
        for v in key.iter().rev() {
            script.push_str(&format!("  push.{v}\n"));
        }
        script.push_str(&format!("  call.{set_fees_root}\n"));
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
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
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
    println!("✓ v6 controller initialized");
    println!("    controller     : {}", controller.to_hex());
    println!("    init tx        : {tx_id}");
    println!("    block          : {height}");
    println!();
    println!("Slots written:");
    println!("  slot 3 (target_weights): DCC, DAG, DCO");
    println!("  slot 4 (fees)          : DCC, DAG, DCO (200/150/100 bps)");

    Ok(())
}
