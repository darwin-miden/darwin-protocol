//! Flow A v3 — atomic deposit via v6 controller's `receive_and_credit`.
//!
//! Mirrors the production single-call path: ONE `call.X` into the
//! controller's compound proc that fuses receive_asset + set_user_position.
//! Exercises slot 10 (user_positions) writes so a follow-up read via
//! get_user_position lands on a populated entry.
//!
//! Storage layout for atomic_deposit_note_v3 (5 felts):
//!   [0] deposit_value
//!   [1] fee_factor
//!   [2] nav_scale
//!   [3] user_id_suffix
//!   [4] user_id_prefix
//!
//! Substitutes the v0.14 `receive_and_credit` MAST root with v0.15's
//! when MIDEN_NETWORK=devnet so the call resolves against the v7
//! controller's procedure surface.

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{Assembler, DefaultSourceManager, Path};
use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{Note, NoteAssets, NoteRecipient, NoteScript, NoteStorage, NoteType};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use rand::RngCore;

// Devnet defaults (2026-06-20 deploy).
const USER_WALLET_HEX_DEVNET: &str = "0x4397442ac860af717888fe90cad00b";
const CONTROLLER_HEX_DEVNET: &str = "0x2388eaea4ce45331214b871755e7b5";
const DETH_FAUCET_HEX_DEVNET: &str = "0xc2c923560dc3cb114ec24ab2291a05";

// Testnet defaults (2026-06-23 v0.15 redeploy).
const USER_WALLET_HEX_TESTNET: &str = "0xd563836959ebc61129e70dd5ab4e1a";
const CONTROLLER_HEX_TESTNET: &str = "0x719bd3a14b42533115b1bcc8e02ea5";
const DETH_FAUCET_HEX_TESTNET: &str = "0xb0411b0e0c4985115c03d034234110";

fn resolve(env_key: &str, devnet: &str, testnet: &str) -> String {
    if let Ok(v) = std::env::var(env_key) {
        return v;
    }
    let net = std::env::var("MIDEN_NETWORK").unwrap_or_else(|_| "testnet".into());
    if net.eq_ignore_ascii_case("devnet") {
        devnet.into()
    } else {
        testnet.into()
    }
}

const RECEIVE_AND_CREDIT_V014: &str =
    "0xeae9e249a88021a2fb6bcae39148f528ee98d5fc884290a42f961b9a536c763e";
const RECEIVE_AND_CREDIT_V015: &str =
    "0x849f526236e9a7ab84a183da209666c3e2839efaeb7d5866a6dca043fdaddc10";

const DEPOSIT_AMOUNT: u64 = 75;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    let endpoint = darwin_protocol_account::miden_endpoint();
    println!("Connecting to Miden ({endpoint:?})…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&endpoint, None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;
    client.sync_state().await?;

    // Build the v3 note script with v0.15 MAST root substitution.
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

    let masm_source = darwin_notes::ATOMIC_DEPOSIT_NOTE_V3_MASM
        .replace(RECEIVE_AND_CREDIT_V014, RECEIVE_AND_CREDIT_V015);
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(masm_source.as_str())?;
    let note_script = NoteScript::new(program);

    let user_wallet = AccountId::from_hex(&resolve(
        "DARWIN_USER_WALLET_HEX",
        USER_WALLET_HEX_DEVNET,
        USER_WALLET_HEX_TESTNET,
    ))?;
    let controller = AccountId::from_hex(&resolve(
        "DARWIN_CONTROLLER_HEX",
        CONTROLLER_HEX_DEVNET,
        CONTROLLER_HEX_TESTNET,
    ))?;
    let deth_faucet = AccountId::from_hex(&resolve(
        "DARWIN_DETH_FAUCET_HEX",
        DETH_FAUCET_HEX_DEVNET,
        DETH_FAUCET_HEX_TESTNET,
    ))?;

    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(
        deth_faucet,
        DEPOSIT_AMOUNT,
    )?)])?;
    let metadata =
        miden_protocol::note::PartialNoteMetadata::new(user_wallet, NoteType::Public);

    // Storage felts: parameters the v3 MASM reads via get_storage.
    //   [0] deposit_value: stand-in for USD-value of the deposit
    //   [1] fee_factor: 9970 bps net (99.7 %)
    //   [2] nav_scale: 1 (placeholder denominator)
    //   [3] user_id_suffix: operator wallet's suffix
    //   [4] user_id_prefix: operator wallet's prefix
    let user_id_suffix = user_wallet.suffix().as_canonical_u64();
    let user_id_prefix = user_wallet.prefix().as_felt().as_canonical_u64();
    println!("user_id_suffix={user_id_suffix} user_id_prefix={user_id_prefix}");
    let storage_felts = vec![
        miden_client::Felt::new(DEPOSIT_AMOUNT).expect("bounded"),
        miden_client::Felt::new(9_970).expect("bounded"),
        miden_client::Felt::new(1).expect("bounded"),
        miden_client::Felt::new(user_id_suffix).expect("bounded"),
        miden_client::Felt::new(user_id_prefix).expect("bounded"),
    ];

    let mut serial = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut serial);
    let serial_num = miden_client::Word::try_from(
        serial
            .chunks_exact(8)
            .map(|c| {
                let mut buf = [0u8; 8];
                buf.copy_from_slice(c);
                miden_client::Felt::new(u64::from_le_bytes(buf) & 0xFFFF_FFFE_FFFF_FFFF)
                    .expect("masked")
            })
            .collect::<Vec<_>>()
            .as_slice(),
    )?;
    let recipient = NoteRecipient::new(
        serial_num,
        note_script.clone(),
        NoteStorage::new(storage_felts)?,
    );
    let note = Note::new(assets, metadata, recipient);
    println!("v3 Note id: {}", note.id());

    println!();
    println!("=== Step 1: user wallet emits the v3 atomic deposit note ===");
    let deploy_r = client
        .execute_transaction(
            user_wallet,
            TransactionRequestBuilder::new()
                .own_output_notes(vec![note.clone()])
                .build()?,
        )
        .await?;
    let deploy_tx = deploy_r.executed_transaction().id();
    println!("Executed user tx: {deploy_tx}");
    let prover = client.prover();
    let deploy_p = client
        .prove_transaction_with(&deploy_r, prover.clone())
        .await?;
    let deploy_h = client
        .submit_proven_transaction(deploy_p, &deploy_r)
        .await?;
    println!("Submitted at block: {deploy_h}");
    client.apply_transaction(&deploy_r, deploy_h).await?;

    println!();
    println!("=== Step 2: controller consumes (receive_and_credit) ===");
    let consume_r = client
        .execute_transaction(
            controller,
            TransactionRequestBuilder::new()
                .input_notes(vec![(note.clone(), None)])
                .build()?,
        )
        .await?;
    let consume_tx = consume_r.executed_transaction().id();
    println!("Executed controller tx: {consume_tx}");
    let consume_p = client.prove_transaction_with(&consume_r, prover).await?;
    let consume_h = client
        .submit_proven_transaction(consume_p, &consume_r)
        .await?;
    println!("Submitted at block: {consume_h}");
    client.apply_transaction(&consume_r, consume_h).await?;

    println!();
    println!("🎯 FLOW A v3 END-TO-END on Miden Devnet:");
    println!("   note id:        {}", note.id());
    println!("   user tx id:     {deploy_tx} (block {deploy_h})");
    println!("   consumer tx id: {consume_tx} (block {consume_h})");
    println!("   {DEPOSIT_AMOUNT} dETH user → atomic v3 note → controller vault");
    println!("   slot 10 user_position written via receive_and_credit");
    Ok(())
}
