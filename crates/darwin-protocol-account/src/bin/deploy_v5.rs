//! Deploy + verify the v5 controller package on Miden testnet.
//!
//! Two phases:
//!
//! 1. **Verify**: load the `.masp`, reconstruct the AccountComponent,
//!    print all MAST roots, sanity-check backward compat with v2/v3/v4
//!    (`receive_asset` must equal `0x75f638c6…`).
//!
//! 2. **Build**: hand the component to `AccountBuilder`, register the
//!    resulting account with the local miden-client store. The account
//!    lands on-chain on its next outbound tx (or via the explicit
//!    bootstrap helper in this binary).
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin deploy_v5 -- \
//!         --masp /tmp/darwin-v5.masp
//!
//! Prereqs:
//!   - ~/.miden/store.sqlite3 + ~/.miden/keystore populated
//!   - testnet rpc reachable (https://rpc.testnet.miden.io)

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::component::AccountComponent;
use miden_client::account::{
    AccountBuilder, AccountType, StorageSlot,
};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::{FilesystemKeyStore, Keystore};
use miden_client_sqlite_store::SqliteStore;
use miden_mast_package::Package;
use miden_client::auth::{AuthSchemeId, AuthSecretKey};
use miden_protocol::account::component::AccountComponentMetadata;
use miden_protocol::account::StorageSlotName;
use miden_assembly::serde::Deserializable;
use miden_client::auth::AuthSingleSig;
use rand::RngCore;

fn parse_args() -> (PathBuf, bool) {
    let mut masp: Option<PathBuf> = None;
    let mut deploy = false;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--masp" | "-m" => masp = Some(PathBuf::from(args.next().expect("--masp value"))),
            "--deploy" => deploy = true,
            _ => {}
        }
    }
    (
        masp.unwrap_or_else(|| PathBuf::from("/tmp/darwin-v5.masp")),
        deploy,
    )
}

const EXPECTED_RECEIVE_ASSET_ROOT: &str =
    "0x75f638c65584d058542bcf4674b066ae394183021bc9b44dc2fdd97d52f9bcfb";

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let (masp_path, deploy) = parse_args();

    println!("Loading v5 controller package from {}", masp_path.display());
    let bytes = std::fs::read(&masp_path)?;
    let mut cursor = std::io::Cursor::new(bytes);
    let package = Package::read_from(&mut cursor)
        .map_err(|e| format!("Package::read_from: {e}"))?;

    let library: miden_assembly::Library = (*package.mast).clone();

    println!();
    println!("v5 controller procedures (MAST roots):");
    let mut receive_asset_root: Option<String> = None;
    for mi in library.module_infos() {
        for (_, pi) in mi.procedures() {
            let bytes: Vec<u8> = pi
                .digest
                .as_elements()
                .iter()
                .flat_map(|f| f.as_canonical_u64().to_le_bytes())
                .collect();
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            let full = format!("0x{hex}");
            println!("  {}::{:<26} {}", mi.path(), pi.name, full);
            if format!("{}", pi.name) == "receive_asset" {
                receive_asset_root = Some(full);
            }
        }
    }

    println!();
    if let Some(root) = &receive_asset_root {
        if root == EXPECTED_RECEIVE_ASSET_ROOT {
            println!(
                "✓ backward-compat: receive_asset MAST matches v2 ({})",
                &root[..18]
            );
        } else {
            println!(
                "✗ backward-compat BROKEN: receive_asset is {} but v2 was {}",
                &root[..18],
                &EXPECTED_RECEIVE_ASSET_ROOT[..18]
            );
            std::process::exit(1);
        }
    }

    // Build the AccountComponent + Account.
    let metadata = AccountComponentMetadata::new("darwin-basket-controller-v5-full-storage");
    // Slots 2 (pool_positions), 3 (target_weights), 4 (fees), and
    // 10 (user_positions) are StorageMaps. The rest are scalar values.
    let map_slots = [2usize, 3, 4, 10];
    let storage_slots: Vec<StorageSlot> = (0..=10)
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
        "AccountComponent built — storage_size={} procedures={}",
        component.storage_size(),
        component.procedures().count(),
    );

    // Generate a fresh Falcon-512 keypair for the controller's auth
    // component. The private key is stored in the local keystore so
    // future admin txs can be signed.
    let auth_scheme = AuthSchemeId::Falcon512Poseidon2;
    let key_pair = AuthSecretKey::new_falcon512_poseidon2();
    let auth_component = AuthSingleSig::new(
        key_pair.public_key().to_commitment(),
        auth_scheme,
    );

    let mut seed = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut seed);

    let account = AccountBuilder::new(seed)
        .account_type(AccountType::Private)
        .with_auth_component(auth_component)
        .with_component(component)
        .build()
        .map_err(|e| format!("build: {e}"))?;

    println!();
    println!("🆕  v5 controller account built");
    println!("    id (hex)    : {}", account.id().to_hex());
    println!("    account_type: {:?}", account.id().account_type());

    if !deploy {
        println!();
        println!("Pass --deploy to register this account in the local store + on testnet.");
        return Ok(());
    }

    let home = std::env::var("HOME")?;
    let store_path = PathBuf::from(format!("{home}/.miden/store.sqlite3"));
    let keystore_path = PathBuf::from(format!("{home}/.miden/keystore"));

    println!();
    println!("Connecting miden-client (testnet)…");
    let store = SqliteStore::new(store_path).await?;
    let keystore = FilesystemKeyStore::new(keystore_path.clone())?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;
    client.sync_state().await?;

    println!("Storing private key in keystore…");
    keystore
        .add_key(&key_pair, account.id())
        .await
        .map_err(|e| format!("add_key: {e}"))?;

    println!("Adding account to local store…");
    client.add_account(&account, false).await?;

    println!();
    println!("✓ v5 controller registered");
    println!("    id (hex)          : {}", account.id().to_hex());
    println!("    id (bech32 hint)  : run `miden client account --show {} | head -2`", account.id().to_hex());
    println!();
    println!("Next: run `deploy_v5_init --controller {}` to write basket configs to slots 3/4.", account.id().to_hex());

    Ok(())
}
