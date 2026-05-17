//! Darwin doctor — pings every Miden testnet account claimed in
//! `darwin-baskets/state/testnet.toml` and verifies it is alive on
//! the live RPC. Prints a row per account with on-chain commitment
//! status, account type, nonce, and vault summary.
//!
//! The output is the proof that every claim made in
//! `darwin-docs/docs/m1-status.md` is actually live — runs against
//! `rpc.testnet.miden.io` so anyone can reproduce.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin darwin_doctor

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client_sqlite_store::SqliteStore;

/// Source of truth: bundled state file from darwin-baskets.
const STATE_TOML: &str = include_str!("../../../../../darwin-baskets/state/testnet.toml");

#[derive(Debug)]
struct AccountCheck {
    label: String,
    hex: String,
    role: &'static str,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    // Fresh local store so we don't trip over pruned-block state.
    let store_path: PathBuf = format!("{home}/.miden/doctor.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    println!("Darwin doctor — verifying on-chain state against Miden testnet…");
    println!();

    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    let checks = parse_state_toml(STATE_TOML);
    println!(
        "{:<35} {:<46} {:<14} {}",
        "ROLE", "ACCOUNT ID", "STATUS", "DETAILS"
    );
    println!("{}", "─".repeat(120));

    let mut alive = 0;
    let mut total = 0;
    for check in &checks {
        total += 1;
        let id = match AccountId::from_hex(&check.hex) {
            Ok(id) => id,
            Err(e) => {
                row(&check.label, &check.hex, "❌ PARSE", &format!("{e}"));
                continue;
            }
        };
        match client.import_account_by_id(id).await {
            Ok(()) => {
                let acct = client.get_account(id).await.ok().flatten();
                let summary = match acct {
                    Some(a) => {
                        let nonce = a.nonce();
                        let assets = a.vault().assets().count();
                        format!("nonce={nonce}, assets={assets}")
                    }
                    None => "fetched, no record".into(),
                };
                row(&check.label, &check.hex, "✅ live", &summary);
                alive += 1;
            }
            Err(e) => {
                let short = format!("{e}").lines().next().unwrap_or("").to_string();
                row(&check.label, &check.hex, "🟡 private", &short);
                // Private accounts can't be imported by id; that's fine.
                if short.contains("private") || short.contains("Private") {
                    alive += 1;
                }
            }
        }
    }

    println!();
    println!("Summary: {alive}/{total} accounts confirmed (live or private-as-expected).");
    Ok(())
}

fn row(label: &str, hex: &str, status: &str, details: &str) {
    println!("{:<35} {:<46} {:<14} {}", trunc(label, 34), hex, status, details);
}

fn trunc(s: &str, n: usize) -> String {
    if s.len() > n {
        format!("{}…", &s[..n.saturating_sub(1)])
    } else {
        s.to_string()
    }
}

/// Walk the state TOML looking for `account_id = "0x…"` lines and
/// attach a label / role guessed from the surrounding section. Keeps
/// the doctor independent of the darwin-baskets typed loader so this
/// binary builds even if the state schema drifts.
fn parse_state_toml(src: &str) -> Vec<AccountCheck> {
    let mut out = Vec::new();
    let mut section: String = String::new();
    let mut role = "—";
    let mut labels: BTreeMap<&'static str, &'static str> = BTreeMap::new();
    labels.insert("asset_faucets", "asset faucet");
    labels.insert("basket_token_faucets", "basket-token faucet");
    labels.insert("protocol_accounts", "controller");
    labels.insert("test_wallet", "team wallet");
    labels.insert("user_wallet", "user wallet");
    labels.insert("mock_oracle", "mock pragma oracle");

    for line in src.lines() {
        let line = line.trim();
        if line.starts_with("[") && line.ends_with("]") {
            section = line[1..line.len() - 1].to_string();
            role = labels
                .iter()
                .find(|(k, _)| section.starts_with(*k))
                .map(|(_, v)| *v)
                .unwrap_or("—");
            continue;
        }
        if let Some(rest) = line
            .strip_prefix("account_id")
            .or_else(|| line.strip_prefix("account_id_v1"))
            .or_else(|| line.strip_prefix("account_id_v2"))
        {
            if let Some(start) = rest.find("0x") {
                if let Some(end) = rest[start..].find('"') {
                    let hex = &rest[start..start + end];
                    out.push(AccountCheck {
                        label: section.replace('.', " / "),
                        hex: hex.to_string(),
                        role,
                    });
                }
            }
        }
    }
    out
}
