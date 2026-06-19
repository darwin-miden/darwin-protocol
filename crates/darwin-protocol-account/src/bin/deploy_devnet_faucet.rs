//! Deploy a Public FungibleFaucet against Miden Devnet via the v0.15
//! `miden-standards::account::faucets::FungibleFaucet` builder.
//!
//! Replaces the v0.14 `miden client mint`-driven flow (the `midenup`
//! toolchain still pins 0.14.0 at the time of writing so the CLI
//! can't emit v0.15 faucets directly).
//!
//! Each call produces ONE faucet account; loop over symbols by re-
//! running the binary with different args. Faucet keys are written
//! into the configured keystore so subsequent `mint` transactions
//! can authenticate as the faucet's owner.
//!
//! Required env:
//!     MIDEN_NETWORK=devnet
//!     HOME=/tmp/miden-devnet-home
//!
//! Required args (positional):
//!     deploy_devnet_faucet <SYMBOL> <DECIMALS> <MAX_SUPPLY>
//!
//! Example:
//!     deploy_devnet_faucet dETH 8 1000000000000

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::component::{
    AuthSingleSig, BurnPolicyConfig, FungibleFaucet, MintPolicyConfig, PolicyRegistration,
    TokenPolicyManager,
};
use miden_client::account::{AccountBuilder, AccountBuilderSchemaCommitmentExt, AccountType};
use miden_client::asset::AssetAmount;
use miden_client::auth::AuthSchemeId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::{FilesystemKeyStore, Keystore};
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::account::auth::AuthSecretKey;
use miden_protocol::asset::TokenSymbol;
// miden-standards is re-exported as `miden_client::account::standards`.
use miden_client::account::standards::faucets::TokenName;
use rand::{RngCore, rngs::OsRng};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err(format!(
            "usage: {} <SYMBOL> <DECIMALS> <MAX_SUPPLY>",
            args[0]
        )
        .into());
    }
    let symbol_str = &args[1];
    let decimals: u8 = args[2].parse()?;
    let max_supply: u64 = args[3].parse()?;

    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();
    std::fs::create_dir_all(&keystore_path)?;

    let endpoint = darwin_protocol_account::miden_endpoint();
    println!("Connecting to Miden ({endpoint:?})…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&endpoint, None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path.clone())?
        .build()
        .await?;

    let network_id = client.network_id().await?;
    println!("Connected. network_id = {network_id:?}");

    // Falcon512+Poseidon2 keypair owns the faucet — needed to sign
    // future mint transactions.
    let auth_scheme = AuthSchemeId::Falcon512Poseidon2;
    let key_pair = AuthSecretKey::new_falcon512_poseidon2();
    let auth_component =
        AuthSingleSig::new(key_pair.public_key().to_commitment(), auth_scheme);

    let symbol = TokenSymbol::new(symbol_str)
        .map_err(|e| format!("invalid token symbol {symbol_str}: {e:?}"))?;
    let name = TokenName::new(&symbol.to_string())
        .map_err(|e| format!("token symbol → name: {e:?}"))?;

    let max_supply_amount = AssetAmount::new(max_supply)
        .map_err(|e| format!("max_supply {max_supply}: {e:?}"))?;

    let faucet = FungibleFaucet::builder()
        .name(name)
        .symbol(symbol)
        .decimals(decimals)
        .max_supply(max_supply_amount)
        .build()
        .map_err(|e| format!("FungibleFaucet::build: {e:?}"))?;

    // Mint + burn policies only — transfer policies install asset
    // callback slots that change the FungibleAsset key derivation and
    // break consumers that use `FungibleAsset::new`.
    let policy_manager = TokenPolicyManager::new()
        .with_mint_policy(MintPolicyConfig::AllowAll, PolicyRegistration::Active)?
        .with_burn_policy(BurnPolicyConfig::AllowAll, PolicyRegistration::Active)?;

    let mut init_seed = [0u8; 32];
    OsRng.fill_bytes(&mut init_seed);

    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::Public)
        .with_auth_component(auth_component)
        .with_component(faucet)
        .with_components(policy_manager)
        .build_with_schema_commitment()?;

    let keystore = FilesystemKeyStore::new(keystore_path)?;
    keystore.add_key(&key_pair, account.id()).await?;

    client.add_account(&account, false).await?;

    let bech32 = account.id().to_bech32(network_id);

    println!();
    println!("✅ FungibleFaucet deployed");
    println!("    symbol             : {symbol_str}");
    println!("    decimals           : {decimals}");
    println!("    max_supply (units) : {max_supply}");
    println!("    AccountId hex      : {}", account.id().to_hex());
    println!("    Address bech32     : {bech32}");

    Ok(())
}
