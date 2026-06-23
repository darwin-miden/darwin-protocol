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

// v0.14 legacy defaults — only useful for localhost or a v0.14 node.
const USER_WALLET_HEX_V014: &str = "0xed3cd5befa3207805f8529207cfc0d";
const CONTROLLER_HEX_V014: &str = "0xa25aa0b00007688024b74b05a52aab";
const DEFAULT_FAUCET_HINT_HEX_V014: &str = "0xfb6811fd6399df206d44f62800620d";

// v0.15 Devnet defaults — operator wallet, v7 controller, DCC faucet.
const USER_WALLET_HEX_DEVNET: &str = "0x4397442ac860af717888fe90cad00b";
const CONTROLLER_HEX_DEVNET: &str = "0x2388eaea4ce45331214b871755e7b5";
const DEFAULT_FAUCET_HINT_HEX_DEVNET: &str = "0x536e8b33e2e10d915bd466faa64099";

// v0.15 Testnet defaults — deployed 2026-06-23.
const USER_WALLET_HEX_TESTNET: &str = "0xd563836959ebc61129e70dd5ab4e1a";
const CONTROLLER_HEX_TESTNET: &str = "0x719bd3a14b42533115b1bcc8e02ea5";
const DEFAULT_FAUCET_HINT_HEX_TESTNET: &str = "0x4eb76287e07e90714a86ae2b89d700";

const BURN_AMOUNT: u64 = 50;

fn is_devnet() -> bool {
    std::env::var("MIDEN_NETWORK")
        .ok()
        .map(|v| v.eq_ignore_ascii_case("devnet"))
        .unwrap_or(false)
}

fn is_testnet() -> bool {
    std::env::var("MIDEN_NETWORK")
        .ok()
        .map(|v| v.eq_ignore_ascii_case("testnet"))
        .unwrap_or(true)
}

fn resolve_hex(env_key: &str, devnet: &str, testnet: &str, legacy: &str) -> String {
    std::env::var(env_key).unwrap_or_else(|_| {
        if is_devnet() {
            devnet.into()
        } else if is_testnet() {
            testnet.into()
        } else {
            legacy.into()
        }
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    println!("Setting up miden-client against testnet…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
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
    // v0.15 hot-patch: the .masm hardcodes the v0.14 receive_asset
    // MAST root (0x75f638c6…); substitute the v0.15 root
    // (0x6170fd6d…) when running on Devnet so the call resolves
    // against the v7 controller.
    const RECEIVE_ASSET_V014: &str =
        "0x75f638c65584d058542bcf4674b066ae394183021bc9b44dc2fdd97d52f9bcfb";
    const RECEIVE_ASSET_V015: &str =
        "0x6170fd6d682d91777b551fd866258f43cc657f1291f8f071500f4e56e9c153da";
    let net = std::env::var("MIDEN_NETWORK").unwrap_or_else(|_| "testnet".into());
    let use_v015 = matches!(net.to_ascii_lowercase().as_str(), "devnet" | "testnet");
    let masm_source = if use_v015 {
        darwin_notes::ATOMIC_REDEEM_NOTE_MASM
            .replace(RECEIVE_ASSET_V014, RECEIVE_ASSET_V015)
    } else {
        darwin_notes::ATOMIC_REDEEM_NOTE_MASM.to_string()
    };
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(masm_source.as_str())?;
    let note_script = NoteScript::new(program);

    let user_wallet_hex = resolve_hex(
        "DARWIN_USER_WALLET_HEX",
        USER_WALLET_HEX_DEVNET,
        USER_WALLET_HEX_TESTNET,
        USER_WALLET_HEX_V014,
    );
    let controller_hex = resolve_hex(
        "DARWIN_CONTROLLER_HEX",
        CONTROLLER_HEX_DEVNET,
        CONTROLLER_HEX_TESTNET,
        CONTROLLER_HEX_V014,
    );
    let user_wallet = AccountId::from_hex(&user_wallet_hex)?;
    let controller = AccountId::from_hex(&controller_hex)?;

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
        .ok_or_else(|| format!("user wallet {user_wallet_hex} not in store after sync"))?;
    let faucet = pick_redeem_faucet(&user_account)?;
    // v0.15: vault.get_balance takes AssetVaultKey (build via
    // FungibleAsset) and returns Result<AssetAmount>. Convert to u64
    // at the boundary so the println formats the number.
    let display_balance: u64 = u64::from(
        user_account
            .vault()
            .get_balance(FungibleAsset::new(faucet, 0)?.vault_key())?,
    );
    println!(
        "Using faucet {} (balance {} ≥ burn {})",
        faucet.to_hex(),
        display_balance,
        BURN_AMOUNT,
    );
    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(
        faucet,
        BURN_AMOUNT,
    )?)])?;
    let metadata = miden_protocol::note::PartialNoteMetadata::new(user_wallet, NoteType::Public);

    let mut serial_num_bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut serial_num_bytes);
    let serial_num = miden_client::Word::try_from(
        serial_num_bytes
            .chunks_exact(8)
            .map(|chunk| {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(chunk);
                miden_client::Felt::new(u64::from_le_bytes(buf) & 0xFFFF_FFFE_FFFF_FFFF).expect("masked to Goldilocks safe range")
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
    // v0.15: vault.get_balance returns Result<AssetAmount>. Pull
    // through a tiny helper so the rest of the lookup keeps comparing
    // u64s.
    fn balance_of(
        account: &miden_client::account::Account,
        faucet: AccountId,
    ) -> u64 {
        FungibleAsset::new(faucet, 0)
            .map(|fa| fa.vault_key())
            .ok()
            .and_then(|k| account.vault().get_balance(k).ok())
            .map(u64::from)
            .unwrap_or(0)
    }

    if let Ok(forced) = std::env::var("DARWIN_FAUCET_HEX") {
        let id = AccountId::from_hex(forced.trim())?;
        let bal = balance_of(user_account, id);
        if bal < BURN_AMOUNT {
            return Err(format!(
                "DARWIN_FAUCET_HEX={forced} but wallet balance is {bal} < {BURN_AMOUNT}",
            )
            .into());
        }
        return Ok(id);
    }

    let hint_hex = if is_devnet() {
        DEFAULT_FAUCET_HINT_HEX_DEVNET
    } else if is_testnet() {
        DEFAULT_FAUCET_HINT_HEX_TESTNET
    } else {
        DEFAULT_FAUCET_HINT_HEX_V014
    };
    let hint = AccountId::from_hex(hint_hex)?;
    if balance_of(user_account, hint) >= BURN_AMOUNT {
        return Ok(hint);
    }

    let candidates: Vec<(AccountId, u64)> = user_account
        .vault()
        .assets()
        .filter_map(|a| match a {
            Asset::Fungible(fa) => Some((fa.faucet_id(), u64::from(fa.amount()))),
            Asset::NonFungible(_) => None,
        })
        .filter(|(_, amt)| *amt >= BURN_AMOUNT)
        .collect();

    if candidates.is_empty() {
        return Err(format!(
            "user wallet has no fungible asset with balance ≥ {BURN_AMOUNT}. \
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
