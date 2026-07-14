//! v10 spike — deploy a basket-token faucet as a NETWORK account.
//!
//! The confidential model (per the grant): a user holds basket TOKENS in
//! their own private Miden account; the collateral vault + supply are
//! public (the fund's AUM, as any ETF publishes) but WHO holds how many
//! tokens is private. To mint those tokens under network execution the
//! basket faucet itself must be a network account, composed from:
//!
//!   - `FungibleFaucet`   → mint_and_send / receive_and_burn.
//!   - `BasicWallet`      → receive_asset, so the faucet can hold the
//!                          dUSDC collateral in its own vault.
//!   - `AuthNetworkAccount` → the NTX builder drives it; only our
//!     deposit/redeem note scripts (allowlisted) can act on it.
//!   - `TokenPolicyManager` with AllowAll mint/burn (permissionless mint;
//!     the note script is the real gate).
//!
//! Composed via `AccountBuilder` directly (same pattern as
//! deploy_devnet_faucet) rather than the `create_fungible_faucet` helper,
//! which rejects NetworkAccount + AuthControlled — the manual compose
//! sidesteps that since AuthNetworkAccount is the sole auth component.
//!
//! Usage:
//!   cargo run -p darwin-protocol-account --bin deploy_v10_faucet -- \
//!       --symbol DCC --deploy [--allow-root 0x…]

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::component::{
    BurnPolicyConfig, FungibleFaucet, MintPolicyConfig, PolicyRegistration, TokenPolicyManager,
};
use miden_client::account::standards::faucets::TokenName;
use miden_client::account::{
    AccountBuilder, AccountBuilderSchemaCommitmentExt, AccountType, NetworkId,
};
use miden_client::asset::AssetAmount;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::asset::TokenSymbol;
use miden_standards::account::auth::AuthNetworkAccount;
use miden_standards::account::wallets::BasicWallet;
use rand::RngCore;
use rand::rngs::OsRng;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let mut symbol = "DCC".to_string();
    let mut deploy = false;
    let mut extra_roots: Vec<String> = Vec::new();
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--symbol" => symbol = args.next().expect("--symbol value"),
            "--deploy" => deploy = true,
            "--allow-root" => extra_roots.push(args.next().expect("--allow-root value")),
            _ => {}
        }
    }

    // Note-script allowlist for the NTX builder. NetworkAccount requires a
    // non-empty set; seed with a placeholder root until the real mint/burn
    // note roots exist (added via --allow-root on later deploys).
    let mut allowed = BTreeSet::new();
    for root_hex in &extra_roots {
        let word = miden_protocol::Word::try_from(root_hex.as_str())
            .map_err(|e| format!("--allow-root {root_hex}: {e}"))?;
        allowed.insert(miden_protocol::note::NoteScriptRoot::from_raw(word));
    }
    if allowed.is_empty() {
        let ph = miden_protocol::Word::from([
            miden_protocol::Felt::new(1).unwrap(),
            miden_protocol::Felt::new(1).unwrap(),
            miden_protocol::Felt::new(1).unwrap(),
            miden_protocol::Felt::new(1).unwrap(),
        ]);
        allowed.insert(miden_protocol::note::NoteScriptRoot::from_raw(ph));
    }

    let token_symbol = TokenSymbol::new(&symbol).map_err(|e| format!("symbol: {e:?}"))?;
    let token_name =
        TokenName::new(&format!("Darwin {symbol}")).map_err(|e| format!("name: {e:?}"))?;
    let max_supply = AssetAmount::new(1_000_000_000_000).map_err(|e| format!("max: {e:?}"))?;

    let faucet = FungibleFaucet::builder()
        .name(token_name)
        .symbol(token_symbol)
        .decimals(6)
        .max_supply(max_supply)
        .build()
        .map_err(|e| format!("FungibleFaucet::build: {e:?}"))?;

    // Mint + burn only (no transfer policy — transfer installs asset
    // callback slots that change FungibleAsset key derivation).
    let policy_manager = TokenPolicyManager::new()
        .with_mint_policy(MintPolicyConfig::AllowAll, PolicyRegistration::Active)?
        .with_burn_policy(BurnPolicyConfig::AllowAll, PolicyRegistration::Active)?;

    let auth = AuthNetworkAccount::with_allowed_notes(allowed)
        .map_err(|e| format!("AuthNetworkAccount: {e:?}"))?;

    let mut init_seed = [0u8; 32];
    OsRng.fill_bytes(&mut init_seed);

    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::Public)
        .with_auth_component(auth)
        .with_component(faucet)
        .with_component(BasicWallet)
        .with_components(policy_manager)
        .build_with_schema_commitment()?;

    println!("🆕  v10 network faucet for {symbol}");
    println!("    id (hex) : {}", account.id().to_hex());
    println!("    bech32   : {}", account.id().to_bech32(NetworkId::Testnet));
    println!("    type     : {:?}", account.id().account_type());

    if !deploy {
        println!();
        println!("Pass --deploy to commit on-chain.");
        return Ok(());
    }

    let home = std::env::var("HOME")?;
    let base = std::env::var("MIDEN_STORE_DIR")
        .unwrap_or_else(|_| format!("{home}/.miden-v10-faucet"));
    std::fs::create_dir_all(&base)?;
    let keystore_path = PathBuf::from(format!("{base}/keystore"));
    std::fs::create_dir_all(&keystore_path)?;
    let store = SqliteStore::new(PathBuf::from(format!("{base}/store.sqlite3"))).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;
    client.sync_state().await?;
    client.add_account(&account, false).await?;
    let tx = TransactionRequestBuilder::new().build()?;
    let tx_id = client.submit_new_transaction(account.id(), tx).await?;

    println!();
    println!("✓ v10 network faucet DEPLOYED");
    println!("    id (hex) : {}", account.id().to_hex());
    println!("    deploy tx: {tx_id:?}");
    println!();
    println!("Next: compile the mint-request note, allowlist its root, redeploy.");

    Ok(())
}
