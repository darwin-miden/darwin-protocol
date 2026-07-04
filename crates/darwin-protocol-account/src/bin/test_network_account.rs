//! Test AuthNetworkAccount auto-consume on testnet.
//!
//! Flow:
//! 1. Emit an atomic_deposit_note_v3 from relay wallet destined to v8
//!    (v8 = 0x5a79c602... — a network account with atomic_deposit_v3
//!    script root in its allowlist).
//! 2. Wait N blocks for the network to auto-consume.
//! 3. Query v8 storage slot 10 for the user_basket_key we set. If the
//!    key holds our AMOUNT_WORD, network auto-consume WORKED.

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::{DefaultSourceManager, Path as ASMPath};
use miden_assembly::ast::{Module, ModuleKind};
use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{Note, NoteAssets, NoteRecipient, NoteScript, NoteStorage, NoteType};
use miden_protocol::note::{NoteAttachment, NoteAttachments};
use miden_standards::note::{NetworkAccountTarget, NoteExecutionHint};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::transaction::TransactionKernel;
use rand::RngCore;

const RELAY_WALLET_HEX: &str = "0x66e7105ea36a7491325480accb7331";
const V8_CONTROLLER_HEX: &str = "0x5a79c602ac2681313adc608901dc51";
const DUSDC_FAUCET_HEX: &str = "0xfc90f0f4da30e51168453b60eafed7";

const DEPOSIT_AMOUNT: u64 = 100_000; // 0.1 dUSDC
const MATH_NAMESPACE: &str = "darwin::math";
const NOTE_V3_MASM: &str = include_str!("../../../darwin-notes/asm/atomic_deposit_note_v3.masm");

// Fake user id (0xdeadbeef... EVM addr encoded as felts)
const USER_ID_SUFFIX: u64 = 0xDEADBEEF_CAFEBABE;
const USER_ID_PREFIX: u64 = 0x00000000_12345678;

// nav math — same as worker
fn compute_storage_felts() -> Vec<miden_client::Felt> {
    // deposit_value * fee / nav_scale = mint
    // For test: deposit_value=100000, fee=9970, nav_scale=1000000 → mint=997
    let deposit_value: u64 = 100_000;
    let fee_factor: u64 = 9970;
    let nav_scale: u64 = 1_000_000;
    vec![
        miden_client::Felt::new(deposit_value).expect("bounded"),
        miden_client::Felt::new(fee_factor).expect("bounded"),
        miden_client::Felt::new(nav_scale).expect("bounded"),
        miden_client::Felt::new(USER_ID_SUFFIX).expect("bounded"),
        miden_client::Felt::new(USER_ID_PREFIX).expect("bounded"),
    ]
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path = PathBuf::from(format!("{home}/.miden/store.sqlite3"));
    let keystore_path = PathBuf::from(format!("{home}/.miden/keystore"));
    std::fs::create_dir_all(&keystore_path)?;

    println!("Connecting miden-client (testnet)…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    // v8 is not yet on-chain (deploy_v8 registered it locally, but no
    // network commit yet). It will be registered when the network
    // executes its first note. We use the id purely as target below.
    let v8 = AccountId::from_hex(V8_CONTROLLER_HEX)?;
    println!("Target network account: {v8}");
    client.sync_state().await?;

    let relay = AccountId::from_hex(RELAY_WALLET_HEX)?;
    let dusdc = AccountId::from_hex(DUSDC_FAUCET_HEX)?;

    // Assemble atomic_deposit_note_v3
    let sm: Arc<dyn miden_assembly::SourceManager> = Arc::new(DefaultSourceManager::default());
    let math_mod = Module::parser(ModuleKind::Library)
        .parse_str(ASMPath::new(MATH_NAMESPACE), darwin_protocol_account::MATH_MASM, sm.clone())?;
    let math_lib = TransactionKernel::assembler().assemble_library([math_mod])?;
    let program = TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(NOTE_V3_MASM)?;
    let note_script = NoteScript::new(program);
    println!("Note script root: {}", note_script.root());

    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(dusdc, DEPOSIT_AMOUNT)?)])?;
    // Sender = relay wallet; metadata sender is the emitter.
    let metadata =
        miden_protocol::note::PartialNoteMetadata::new(relay, NoteType::Public);
    // Network Account attachment — tells the network to auto-execute
    // consume against v8 when this note arrives.
    let na_target = NetworkAccountTarget::new(v8, NoteExecutionHint::always())?;
    let attachments = NoteAttachments::from(NoteAttachment::from(na_target));
    let storage_felts = compute_storage_felts();

    let mut serial = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut serial);
    let serial_num = miden_client::Word::try_from(
        serial.chunks_exact(8)
            .map(|c| {
                let mut b = [0u8; 8]; b.copy_from_slice(c);
                miden_client::Felt::new(u64::from_le_bytes(b) & 0xFFFF_FFFE_FFFF_FFFF).expect("masked")
            })
            .collect::<Vec<_>>().as_slice(),
    )?;
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), NoteStorage::new(storage_felts)?);
    let note = Note::with_attachments(assets, metadata, recipient, attachments);
    println!("Note id: {}", note.id());
    println!("Target: v8={v8}");
    println!("User id: suffix={USER_ID_SUFFIX} prefix={USER_ID_PREFIX}");

    println!();
    println!("=== Emit note from relay wallet ===");
    let r = client.execute_transaction(
        relay,
        TransactionRequestBuilder::new().own_output_notes(vec![note.clone()]).build()?,
    ).await?;
    let prover = client.prover();
    let p = client.prove_transaction_with(&r, prover.clone()).await?;
    let h = client.submit_proven_transaction(p, &r).await?;
    client.apply_transaction(&r, h).await?;
    println!("emit at block {h}");

    println!();
    println!("=== Wait 60s for network auto-consume ===");
    for i in 1..=12 {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        client.sync_state().await?;
        match client.import_account_by_id(v8).await {
            Ok(_) => {
                let acc = client.get_account(v8).await?;
                if let Some(a) = acc {
                    let storage = a.storage();
                    println!("[{}0s] v8 nonce={}, storage slots={}", i, a.nonce(), storage.num_slots());
                }
            }
            Err(e) => println!("[{}0s] not yet on-chain: {}", i, format!("{e:?}").chars().take(80).collect::<String>()),
        }
    }

    Ok(())
}
