//! Deploy the permissionless dUSDC dispenser: a NETWORK account (BasicWallet +
//! AuthNetworkAccount) that holds bridged Epoch dUSDC and allowlists exactly ONE
//! note script — the drip note. Anyone can then create a drip request; the
//! network executes it against this account and it pays out a fixed 5 dUSDC to
//! the requester from its own vault. No operator key, no server, no reserve
//! wallet — a real permissionless faucet.
//!
//! Env:  HOME=/Users/eden/data/darwin/.v015-asset-faucets   MIDEN_NETWORK=testnet
//! Run:  cargo run --release -p darwin-protocol-account --bin deploy_dispenser
//!
//! Fund it AFTER with a small test amount (transfer dUSDC from the existing
//! wallet dispenser), then exercise a drip end-to-end via the CLI.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{DefaultSourceManager, Path as AsmPath};
use miden_client::account::{
    AccountBuilder, AccountBuilderSchemaCommitmentExt, AccountId, AccountType, NetworkId,
};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::NoteScript;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::transaction::TransactionKernel;
use miden_standards::account::auth::AuthNetworkAccount;
use miden_standards::account::wallets::BasicWallet;
use miden_standards::note::{P2idNote, P2ideNote};
use rand::RngCore;
use rand::rngs::OsRng;

// The bridged Epoch dUSDC faucet — the token the dispenser pays out.
const DUSDC_FAUCET_HEX: &str = "0xfc90f0f4da30e51168453b60eafed7";
const DRIP_AMOUNT: u64 = 5_000_000; // 5 dUSDC (6-dec)

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    // 1. dUSDC faucet id → prefix/suffix felts for the drip script.
    let dusdc = AccountId::from_hex(DUSDC_FAUCET_HEX)?;
    let prefix: u64 = dusdc.prefix().as_felt().as_canonical_u64();
    let suffix: u64 = dusdc.suffix().as_canonical_u64();
    println!("dUSDC faucet felts: prefix={prefix} suffix={suffix}");

    // 2. Assemble the drip note with the real felts → its NoteScript root.
    let sm: Arc<dyn miden_assembly::SourceManager> = Arc::new(DefaultSourceManager::default());
    let wallet_module = Module::parser(ModuleKind::Library).parse_str(
        AsmPath::new("miden::standards::wallets::basic"),
        darwin_notes::STD_BASIC_WALLET_MASM,
        sm.clone(),
    )?;
    let wallet_lib = TransactionKernel::assembler().assemble_library([wallet_module])?;

    let src = darwin_notes::DRIP_NOTE_MASM
        .replace("{{DRIP_AMOUNT}}", &DRIP_AMOUNT.to_string())
        .replace("{{DUSDC_FAUCET_PREFIX}}", &prefix.to_string())
        .replace("{{DUSDC_FAUCET_SUFFIX}}", &suffix.to_string());
    let program = TransactionKernel::assembler()
        .with_static_library(wallet_lib.as_ref())?
        .assemble_program(&src)?;
    let drip_root = NoteScript::new(program).root();
    println!("drip NoteScript root: {drip_root:?}");

    // 3. Build the NETWORK-account dispenser: BasicWallet (hold + move assets),
    //    AuthNetworkAccount allowlisting ONLY the drip script.
    let mut allowed = BTreeSet::new();
    allowed.insert(drip_root.clone());
    // Also allowlist P2ID/P2IDE so the network consumes funding notes into the
    // dispenser's vault (receiving assets is safe — only the drip pays out).
    allowed.insert(P2idNote::script_root());
    allowed.insert(P2ideNote::script_root());
    let auth = AuthNetworkAccount::with_allowed_notes(allowed)
        .map_err(|e| format!("AuthNetworkAccount: {e:?}"))?;

    let mut init_seed = [0u8; 32];
    OsRng.fill_bytes(&mut init_seed);
    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::Public)
        .with_auth_component(auth)
        .with_component(BasicWallet)
        .build_with_schema_commitment()?;

    println!("🆕  dUSDC dispenser (network account)");
    println!("    id (hex) : {}", account.id().to_hex());
    println!("    bech32   : {}", account.id().to_bech32(NetworkId::Testnet));

    // 4. Deploy on-chain into the durable faucet store (HOME-based).
    let home = std::env::var("HOME")?;
    let base = format!("{home}/.miden");
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
    println!("✓ dispenser DEPLOYED");
    println!("    id (hex)  : {}", account.id().to_hex());
    println!("    deploy tx : {tx_id:?}");
    println!("    drip root : {drip_root:?}");
    println!();
    println!("Next: fund it with dUSDC, then create a drip request note.");
    Ok(())
}
