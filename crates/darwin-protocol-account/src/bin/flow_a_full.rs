//! Full Flow A on Miden testnet, end-to-end in one process.
//!
//! Two transactions submitted back-to-back:
//!   1. User wallet → output note carrying 100 dETH for the controller.
//!      The note script is `ATOMIC_DEPOSIT_NOTE_MASM` and embeds
//!      `darwin::math::felt_div` (real u64 division).
//!   2. Real-bodies controller → consumes the note (unauthenticated),
//!      so the dETH moves into the controller's vault and the note's
//!      script runs on-chain.
//!
//! Builds the Note in-memory and passes it directly to both
//! transactions, avoiding any local sqlite sync round-trip.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin flow_a_full

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

// v0.14 testnet defaults.
const USER_WALLET_HEX_V014: &str = "0xed3cd5befa3207805f8529207cfc0d";
// v2 real-bodies controller on testnet: exposes `receive_asset`.
const CONTROLLER_HEX_V014: &str = "0xa25aa0b00007688024b74b05a52aab";
const DETH_FAUCET_HEX_V014: &str = "0xa095d9b3831e96206ff70c2218a6a9";

// v0.15 Devnet defaults — operator wallet, v7 controller, DETH faucet
// deployed 2026-06-20.
const USER_WALLET_HEX_V015: &str = "0x4397442ac860af717888fe90cad00b";
const CONTROLLER_HEX_V015: &str = "0x2388eaea4ce45331214b871755e7b5";
const DETH_FAUCET_HEX_V015: &str = "0xc2c923560dc3cb114ec24ab2291a05";

const DEPOSIT_AMOUNT: u64 = 100;

fn is_devnet() -> bool {
    std::env::var("MIDEN_NETWORK")
        .ok()
        .map(|v| v.eq_ignore_ascii_case("devnet"))
        .unwrap_or(false)
}

fn resolve_hex(env_key: &str, devnet: &str, testnet: &str) -> String {
    std::env::var(env_key).unwrap_or_else(|_| {
        if is_devnet() {
            devnet.into()
        } else {
            testnet.into()
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
    // Use the TransactionKernel assembler so the note's
    // `use miden::protocol::active_note` etc. resolve.
    //
    // v0.15 hot-patch: the .masm file hardcodes the v0.14 receive_asset
    // MAST root (0x75f638c6…). Under Devnet we substitute it for the
    // v0.15 root (0x6170fd6d…) so the call resolves against the
    // v7 controller's procedure surface.
    const RECEIVE_ASSET_V014: &str =
        "0x75f638c65584d058542bcf4674b066ae394183021bc9b44dc2fdd97d52f9bcfb";
    const RECEIVE_ASSET_V015: &str =
        "0x6170fd6d682d91777b551fd866258f43cc657f1291f8f071500f4e56e9c153da";
    let masm_source = if is_devnet() {
        darwin_notes::ATOMIC_DEPOSIT_NOTE_MASM
            .replace(RECEIVE_ASSET_V014, RECEIVE_ASSET_V015)
    } else {
        darwin_notes::ATOMIC_DEPOSIT_NOTE_MASM.to_string()
    };
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(masm_source.as_str())?;
    let note_script = NoteScript::new(program);

    let user_wallet_hex = resolve_hex(
        "DARWIN_USER_WALLET_HEX",
        USER_WALLET_HEX_V015,
        USER_WALLET_HEX_V014,
    );
    let controller_hex = resolve_hex(
        "DARWIN_CONTROLLER_HEX",
        CONTROLLER_HEX_V015,
        CONTROLLER_HEX_V014,
    );
    let deth_faucet_hex = resolve_hex(
        "DARWIN_DETH_FAUCET_HEX",
        DETH_FAUCET_HEX_V015,
        DETH_FAUCET_HEX_V014,
    );
    let user_wallet = AccountId::from_hex(&user_wallet_hex)?;
    let controller = AccountId::from_hex(&controller_hex)?;
    let deth_faucet = AccountId::from_hex(&deth_faucet_hex)?;
    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(
        deth_faucet,
        DEPOSIT_AMOUNT,
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
    // Parameterised note storage: [deposit_value, fee_factor, nav_scale].
    // The atomic deposit note reads these via `active_note::get_storage`
    // and computes deposit_value * fee_factor / nav_scale via
    // darwin::math::felt_div on-chain. Demo defaults below match the
    // earlier hard-coded values so observable behaviour is unchanged.
    let storage_felts = vec![
        miden_client::Felt::new(200_000_000_000).expect("bounded"), // deposit_value
        miden_client::Felt::new(9_970).expect("bounded"),           // fee_factor (99.7%)
        miden_client::Felt::new(10_000_000_000).expect("bounded"),  // nav_scale
    ];
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), NoteStorage::new(storage_felts)?);
    let note = Note::new(assets, metadata, recipient);
    println!("Constructed Note id: {}", note.id());

    // -- Step 1: user wallet submits a tx that emits this note ----------
    println!();
    println!("=== Step 1: user wallet emits the atomic deposit note ===");
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
    println!("Local store updated.");

    // -- Step 2: controller submits a tx that consumes the note --------
    println!();
    println!("=== Step 2: controller consumes the atomic deposit note ===");
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
    println!("🎯 FLOW A END-TO-END on Miden testnet:");
    println!("   note id:        {}", note.id());
    println!("   user tx id:     {deploy_tx_id} (block {deploy_height})");
    println!("   consumer tx id: {consume_tx_id} (block {consume_height})");
    println!("   100 dETH moved from user wallet → atomic deposit note → controller vault.");
    println!("   darwin::math::felt_div ran on-chain (miden::core::math::u64::div event).");

    Ok(())
}
