//! Deploy the Darwin controller as a NETWORK account on Miden testnet.
//!
//! A network account (per the node's ntx-builder doc) is a fully public
//! account whose auth component is `AuthNetworkAccount`: instead of a
//! signature gate it carries an allowlist of note script roots. The
//! testnet's Network Transaction Builder detects such accounts by their
//! standardized allowlist storage slot, spawns an actor for them, and
//! consumes any notes targeting the account whose script root is in the
//! allowlist — i.e. the NETWORK drives the account state; no operator
//! key, no relay, no NoAuth free-for-all.
//!
//! Phases:
//!   1. Verify: load the controller .masp, print MAST roots.
//!   2. Build: controller component + AuthNetworkAccount({P2ID root}).
//!   3. --deploy: register locally + submit an empty transaction to
//!      commit the account on-chain (nonce 0 → 1, same trick the
//!      miden-cli `new-account --deploy` uses). AuthNetworkAccount's
//!      epilogue passes: zero consumed notes, zero tx script.
//!   4. --send-test <FAUCET_HEX> <AMOUNT> --from <SENDER_HEX>: send a
//!      P2ID note carrying a NetworkAccountTarget attachment at the
//!      network account, then poll `network-note-status`-style via the
//!      client until the NTB consumes it.
//!
//! Usage:
//!   cargo run -p darwin-protocol-account --bin deploy_v9_network -- \
//!       --masp /tmp/darwin-v6-fee-routing-controller.masp --deploy

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::serde::Deserializable;
use miden_client::account::component::AccountComponent;
use miden_client::account::{AccountBuilder, AccountType, StorageSlot};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_mast_package::Package;
use miden_protocol::account::StorageSlotName;
use miden_protocol::account::component::AccountComponentMetadata;
use miden_standards::account::auth::AuthNetworkAccount;
use miden_standards::note::P2idNote;
use rand::RngCore;

struct Args {
    masp: PathBuf,
    deploy: bool,
    extra_roots: Vec<String>,
}

fn parse_args() -> Args {
    let mut masp: Option<PathBuf> = None;
    let mut deploy = false;
    let mut extra_roots = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--masp" | "-m" => masp = Some(PathBuf::from(args.next().expect("--masp value"))),
            "--deploy" => deploy = true,
            // Additional note script roots to allowlist (e.g. the
            // atomic_deposit_note root once we wire position writes).
            "--allow-root" => extra_roots.push(args.next().expect("--allow-root value")),
            _ => {}
        }
    }
    Args {
        masp: masp
            .unwrap_or_else(|| PathBuf::from("/tmp/darwin-v6-fee-routing-controller.masp")),
        deploy,
        extra_roots,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = parse_args();

    println!("Loading controller package from {}", args.masp.display());
    let bytes = std::fs::read(&args.masp)?;
    let mut cursor = std::io::Cursor::new(bytes);
    let package =
        Package::read_from(&mut cursor).map_err(|e| format!("Package::read_from: {e}"))?;
    let library: miden_assembly::Library = (*package.mast).clone();

    println!();
    println!("controller procedures (MAST roots):");
    for mi in library.module_infos() {
        for (_, pi) in mi.procedures() {
            let bytes: Vec<u8> = pi
                .digest
                .as_elements()
                .iter()
                .flat_map(|f| f.as_canonical_u64().to_le_bytes())
                .collect();
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            println!("  {}::{:<26} 0x{hex}", mi.path(), pi.name);
        }
    }

    // Controller component — same slot layout as v7/v8: maps at 2
    // (pool_positions), 3 (target_weights), 4 (fees), 10
    // (user_positions); scalars elsewhere.
    let metadata = AccountComponentMetadata::new("darwin-basket-controller-v9-network");
    let map_slots = [2usize, 3, 4, 10];
    let storage_slots: Vec<StorageSlot> = (0..=11)
        .map(|i| {
            let name = StorageSlotName::new(format!("darwin::slot_{i}")).expect("slot name");
            if map_slots.contains(&i) {
                StorageSlot::with_empty_map(name)
            } else {
                StorageSlot::with_empty_value(name)
            }
        })
        .collect();
    let component = AccountComponent::new(library, storage_slots, metadata)
        .map_err(|e| format!("AccountComponent::new: {e}"))?;
    println!();
    println!(
        "controller component built — storage_size={} procedures={}",
        component.storage_size(),
        component.procedures().count(),
    );

    // Network-account auth: allowlist of note script roots the NTB may
    // consume against this account. Start with P2ID (asset deposits into
    // the controller vault); extend with --allow-root for the atomic
    // deposit note once position writes go through the network too.
    let mut allowed = BTreeSet::new();
    allowed.insert(P2idNote::script_root());
    println!();
    println!("note-script allowlist:");
    println!("  P2ID  {}", P2idNote::script_root());
    for root_hex in &args.extra_roots {
        let word = miden_protocol::Word::try_from(root_hex.as_str())
            .map_err(|e| format!("--allow-root {root_hex}: {e}"))?;
        let root = miden_protocol::note::NoteScriptRoot::from_raw(word);
        println!("  extra {root}");
        allowed.insert(root);
    }
    let auth = AuthNetworkAccount::with_allowed_notes(allowed)
        .map_err(|e| format!("AuthNetworkAccount: {e}"))?;

    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);

    let account = AccountBuilder::new(seed)
        .account_type(AccountType::Public)
        .with_auth_component(auth)
        .with_component(component)
        .build()
        .map_err(|e| format!("build: {e}"))?;

    println!();
    println!("🆕  v9-NETWORK controller account built");
    println!("    id (hex)    : {}", account.id().to_hex());
    println!("    account_type: {:?}", account.id().account_type());

    if !args.deploy {
        println!();
        println!("Pass --deploy to register + commit the account on-chain.");
        return Ok(());
    }

    // Dedicated store dir: the shared ~/.miden store was created by a
    // different client build (migration hashes mismatch).
    let home = std::env::var("HOME")?;
    let base = std::env::var("MIDEN_STORE_DIR")
        .unwrap_or_else(|_| format!("{home}/.miden-v9-network"));
    std::fs::create_dir_all(&base)?;
    let store_path = PathBuf::from(format!("{base}/store.sqlite3"));
    let keystore_path = PathBuf::from(format!("{base}/keystore"));
    std::fs::create_dir_all(&keystore_path)?;

    println!();
    println!("Connecting miden-client (testnet)…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;
    client.sync_state().await?;

    // No keystore entry needed: AuthNetworkAccount has no signing key.
    println!("Adding account to local store…");
    client.add_account(&account, false).await?;

    // Commit on-chain: an empty transaction request. Execution bumps the
    // nonce 0 → 1 which deploys the account — the same mechanism the
    // miden-cli uses for `--deploy`. AuthNetworkAccount's epilogue check
    // passes vacuously (no consumed notes, no tx script).
    println!("Submitting deploy transaction (nonce 0 → 1)…");
    let tx_request = TransactionRequestBuilder::new()
        .build()
        .map_err(|e| format!("deploy tx build: {e}"))?;
    let tx_id = client
        .submit_new_transaction(account.id(), tx_request)
        .await
        .map_err(|e| format!("submit deploy tx: {e}"))?;

    println!();
    println!("✓ v9-network controller DEPLOYED");
    println!("    id (hex) : {}", account.id().to_hex());
    println!("    deploy tx: {tx_id:?}");
    println!();
    println!("The testnet NTX builder should now detect the allowlist slot and");
    println!("spawn an actor for this account once the deploy tx is committed.");
    println!("Test with: send a P2ID note (dUSDC) targeting this account, then");
    println!("check `miden-client network-note-status <note_id>`.");

    Ok(())
}
