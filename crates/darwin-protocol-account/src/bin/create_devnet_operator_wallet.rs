//! Create a fresh Public BasicWallet under Miden v0.15 client.
//!
//! Defaults to **Devnet** (`MIDEN_NETWORK=devnet`) so the printed
//! address can be pasted straight into `faucet.devnet.miden.io` to
//! receive initial mBND for gas. Override with `MIDEN_NETWORK=testnet`
//! or `MIDEN_NETWORK=localhost` to target other networks.
//!
//! Per [[relay-wallet-migration]]: operator wallets MUST be Public —
//! Private drifted unrecoverably on testnet and forced a full
//! redeploy. The same constraint applies to the v0.15 deployment.
//!
//! Output: AccountId hex + bech32 address. Feed the bech32 into the
//! faucet UI to drip 100 mBND.
//!
//! Usage:
//!     MIDEN_NETWORK=devnet \
//!     HOME=/tmp/miden-devnet-home \
//!     cargo run --release -p darwin-protocol-account --bin create_devnet_operator_wallet

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::component::{AuthSingleSig, BasicWallet};
use miden_client::account::{
    AccountBuilder, AccountBuilderSchemaCommitmentExt, AccountType,
};
use miden_client::auth::AuthSchemeId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::{FilesystemKeyStore, Keystore};
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::account::auth::AuthSecretKey;
use rand::{RngCore, rngs::OsRng};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();
    std::fs::create_dir_all(&keystore_path)?;

    let endpoint = darwin_protocol_account::miden_endpoint();
    println!("Connecting to Miden ({endpoint:?})…");
    let store = SqliteStore::new(store_path.clone()).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&endpoint, None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path.clone())?
        .build()
        .await?;

    // Probe network so the gRPC handshake actually happens before we
    // start consuming local entropy — surfaces an unreachable RPC
    // before the wallet bytes are written to disk.
    let network_id = client.network_id().await?;
    println!("Connected. network_id = {network_id:?}");

    // Falcon512+Poseidon2 is the v0.15 default for Miden-native wallets.
    let auth_scheme = AuthSchemeId::Falcon512Poseidon2;
    let key_pair = AuthSecretKey::new_falcon512_poseidon2();
    let auth_component =
        AuthSingleSig::new(key_pair.public_key().to_commitment(), auth_scheme);

    // Use OS entropy directly — `Client::rng()` isn't exposed publicly
    // in 0.15 (the test_utils variant we used as the reference is
    // gated behind cfg(test)).
    let mut init_seed = [0u8; 32];
    OsRng.fill_bytes(&mut init_seed);

    // Public storage mode — operator wallets must stay recoverable.
    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::Public)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .build_with_schema_commitment()?;

    let keystore = FilesystemKeyStore::new(keystore_path.clone())?;
    keystore.add_key(&key_pair, account.id()).await?;

    client.add_account(&account, false).await?;

    let bech32 = account.id().to_bech32(network_id);

    println!();
    println!("✅ Fresh Public BasicWallet created on Devnet.");
    println!("   AccountId hex     : {}", account.id().to_hex());
    println!("   Address (bech32)  : {bech32}");
    println!("   Store             : {}", store_path.display());
    println!("   Keystore          : {}", keystore_path.display());
    println!();
    println!("Next: visit https://faucet.devnet.miden.io, paste the");
    println!("bech32 address above, request 100 mBND. Once dripped,");
    println!("`darwin_doctor` will see the balance and deploy_v7");
    println!("--deploy can use this wallet for gas.");

    Ok(())
}
