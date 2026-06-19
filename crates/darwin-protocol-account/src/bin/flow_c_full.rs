//! Full Flow C Path 2 (Miden-native redeem) on Miden testnet,
//! end-to-end in one process.
//!
//! Symmetric to `flow_a_full`:
//!   1. User wallet emits a RedeemNote carrying `BURN_AMOUNT` of the
//!      DCC basket token, with the v2 controller as the target. The
//!      note script is `ATOMIC_REDEEM_NOTE_MASM`, which runs
//!      `darwin::math::felt_div` then hands the DCC to the controller
//!      via `call.<receive_asset_root>`.
//!   2. v2 controller consumes the note. The DCC lands in the
//!      controller's vault — the on-chain effect of "burning" the
//!      user's basket tokens (they leave circulation).
//!
//! In M2 the controller chains in an explicit basket-faucet `burn`
//! call so the supply decrements too, and emits P2ID output notes
//! carrying the released underlyings back to the user wallet. The
//! atomic version here proves the burn-and-absorb half on-chain.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin flow_c_full

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{Assembler, DefaultSourceManager, Path};
use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{
    Note, NoteAssets, NoteMetadata, NoteRecipient, NoteScript, NoteStorage, NoteType,
};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use rand::RngCore;

const USER_WALLET_HEX: &str = "0xed3cd5befa3207805f8529207cfc0d";
const REAL_BODIES_CONTROLLER_HEX: &str = "0xa25aa0b00007688024b74b05a52aab";
// Basket-token faucet. M1 deploys: DCC 0x2066f2da…, DAG 0xfb6811fd…,
// DCO 0xbe4efc67…. The atomic redeem path is faucet-agnostic — the
// note script doesn't care which faucet it's burning, only that the
// user wallet actually holds enough units of it. Override the default
// hint via env `DARWIN_FAUCET_HEX` if you want to force a specific
// faucet; otherwise the binary discovers a fungible asset with
// balance >= BURN_AMOUNT from the user wallet's vault.
const DEFAULT_FAUCET_HINT_HEX: &str = "0xfb6811fd6399df206d44f62800620d";
const BURN_AMOUNT: u64 = 50;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    println!("Setting up miden-client against testnet…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    // Build the NoteScript with darwin::math attached.
    let core_lib = miden_core_lib::CoreLibrary::default();
    let sm: Arc<dyn miden_assembly::SourceManager> = Arc::new(DefaultSourceManager::default());
    let math_module = Module::parser(ModuleKind::Library).parse_str(
        Path::new("darwin::math"),
        darwin_protocol_account::MATH_MASM,
        sm.clone(),
    )?;
    let math_lib = Assembler::default()
        .with_static_library(core_lib.as_ref())?
        .assemble_library([math_module])?;
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(darwin_notes::ATOMIC_REDEEM_NOTE_MASM)?;
    let note_script = NoteScript::new(program);

    let user_wallet = AccountId::from_hex(USER_WALLET_HEX)?;
    let controller = AccountId::from_hex(REAL_BODIES_CONTROLLER_HEX)?;

    // Sync so the local store has the freshest vault state. The user
    // wallet is private (Falcon-512), so we can't import_account_by_id
    // for it — it's already locally tracked.
    println!("Syncing wallet state…");
    client.sync_state().await?;

    // Faucet discovery: env override > hint balance > any vault asset
    // with balance >= BURN_AMOUNT. This makes the binary self-bootstrap
    // — no manual `miden client account --show` step.
    let user_account = client
        .get_account(user_wallet)
        .await?
        .ok_or_else(|| format!("user wallet {USER_WALLET_HEX} not in store after sync"))?;
    let faucet = pick_redeem_faucet(&user_account)?;
    println!(
        "Using faucet {} (balance {} ≥ burn {})",
        faucet.to_hex(),
        user_account.vault().get_balance(faucet)?,
        BURN_AMOUNT,
    );
    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(
        faucet,
        BURN_AMOUNT,
    )?)])?;
    let metadata = NoteMetadata::new(user_wallet, NoteType::Public);

    let mut serial_num_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut serial_num_bytes);
    let serial_num = miden_client::Word::try_from(
        serial_num_bytes
            .chunks_exact(8)
            .map(|chunk| {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(chunk);
                miden_client::Felt::new(u64::from_le_bytes(buf).expect("bounded") & 0xFFFF_FFFE_FFFF_FFFF).expect("masked to Goldilocks safe range")
            })
            .collect::<Vec<_>>()
            .as_slice(),
    )?;
    // Parameterised note storage for atomic_redeem_note:
    // [burn_amount, gross_release_factor, scale]. The note reads these
    // via active_note::get_storage and computes
    //   release_value = burn_amount * gross_release_factor / scale
    // via darwin::math::felt_div. Empty storage triggers divide-by-zero.
    let storage_felts = vec![
        miden_client::Felt::new(BURN_AMOUNT).expect("bounded"),
        miden_client::Felt::new(9_970).expect("bounded"),   // gross_release_factor (99.7% net of 30 bps redeem fee)
        miden_client::Felt::new(1).expect("bounded"),       // scale (placeholder denominator)
    ];
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), NoteStorage::new(storage_felts)?);
    let note = Note::new(assets, metadata, recipient);
    println!("Constructed RedeemNote id: {}", note.id());

    // -- Step 1: user wallet emits the redeem note --------------------
    println!();
    println!("=== Step 1: user wallet emits the atomic redeem note ===");
    let deploy_request = TransactionRequestBuilder::new()
        .own_output_notes(vec![note.clone()])
        .build()?;
    let deploy_result = client.execute_transaction(user_wallet, deploy_request).await?;
    let deploy_tx_id = deploy_result.executed_transaction().id();
    println!("Executed user tx: {deploy_tx_id}");
    let prover = client.prover();
    let deploy_proven = client.prove_transaction_with(&deploy_result, prover.clone()).await?;
    let deploy_height = client.submit_proven_transaction(deploy_proven, &deploy_result).await?;
    println!("Submitted at block: {deploy_height}");
    client.apply_transaction(&deploy_result, deploy_height).await?;

    // -- Step 2: controller consumes the redeem note ------------------
    println!();
    println!("=== Step 2: controller consumes the atomic redeem note ===");
    let consume_request = TransactionRequestBuilder::new()
        .input_notes(vec![(note.clone(), None)])
        .build()?;
    let consume_result = client.execute_transaction(controller, consume_request).await?;
    let consume_tx_id = consume_result.executed_transaction().id();
    println!("Executed controller tx: {consume_tx_id}");
    let consume_proven = client.prove_transaction_with(&consume_result, prover).await?;
    let consume_height = client.submit_proven_transaction(consume_proven, &consume_result).await?;
    println!("Submitted at block: {consume_height}");
    client.apply_transaction(&consume_result, consume_height).await?;

    println!();
    println!("🎯 FLOW C PATH 2 END-TO-END on Miden testnet:");
    println!("   note id:        {}", note.id());
    println!("   user tx id:     {deploy_tx_id} (block {deploy_height})");
    println!("   consumer tx id: {consume_tx_id} (block {consume_height})");
    println!(
        "   {BURN_AMOUNT} DCC moved from user wallet → atomic redeem note → controller vault."
    );
    println!("   darwin::math::felt_div ran on-chain inside the controller tx context.");
    println!();
    println!("Next iteration (M2): the controller chains in a basket-faucet `burn` call");
    println!("so DCC supply decrements + emits P2ID output notes carrying the released");
    println!("underlyings back to the user wallet.");

    Ok(())
}

fn pick_redeem_faucet(
    user_account: &miden_client::account::Account,
) -> Result<AccountId, Box<dyn std::error::Error>> {
    if let Ok(forced) = std::env::var("DARWIN_FAUCET_HEX") {
        let id = AccountId::from_hex(forced.trim())?;
        let bal = user_account.vault().get_balance(id)?;
        if bal < BURN_AMOUNT {
            return Err(format!(
                "DARWIN_FAUCET_HEX={forced} but wallet balance is {bal} < {BURN_AMOUNT}",
            )
            .into());
        }
        return Ok(id);
    }

    let hint = AccountId::from_hex(DEFAULT_FAUCET_HINT_HEX)?;
    if user_account.vault().get_balance(hint).unwrap_or(0) >= BURN_AMOUNT {
        return Ok(hint);
    }

    let candidates: Vec<(AccountId, u64)> = user_account
        .vault()
        .assets()
        .filter_map(|a| match a {
            Asset::Fungible(fa) => Some((fa.faucet_id(), fa.amount())),
            Asset::NonFungible(_) => None,
        })
        .filter(|(_, amt)| *amt >= BURN_AMOUNT)
        .collect();

    if candidates.is_empty() {
        return Err(format!(
            "user wallet {USER_WALLET_HEX} has no fungible asset with balance ≥ {BURN_AMOUNT}. \
             Run a deposit first (e.g. `flow_a_full`) so the controller mints basket-tokens to it."
        )
        .into());
    }

    let (best, _) = candidates
        .iter()
        .min_by_key(|(_, amt)| *amt)
        .copied()
        .unwrap();
    Ok(best)
}
