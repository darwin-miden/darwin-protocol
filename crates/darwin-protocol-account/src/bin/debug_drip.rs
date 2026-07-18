//! Debug the drip note by consuming it LOCALLY against the wallet dispenser
//! (a regular account with a key + BasicWallet + dUSDC in its vault). Running
//! it locally surfaces the MASM runtime error directly — the NTX builder path
//! is opaque. Once this succeeds locally, the network path will too.

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{DefaultSourceManager, Path as AsmPath};
use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{
    Note, NoteAssets, NoteRecipient, NoteScript, NoteStorage, NoteTag, NoteType,
    PartialNoteMetadata,
};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::transaction::TransactionKernel;
use miden_standards::note::P2idNoteStorage;
use rand::RngCore;

const WALLET_DISP: &str = "0xaea2b3093957cc7163f64a64a297c6"; // regular acct, key + dUSDC
const PAYOUT_TO: &str = "0x6e3ecd775ce8ae910a0b509e098059"; // arbitrary payout recipient
const DUSDC_FAUCET_HEX: &str = "0xfc90f0f4da30e51168453b60eafed7";
const DRIP_AMOUNT: u64 = 5_000_000;

fn rand_word() -> Result<miden_client::Word, Box<dyn std::error::Error>> {
    let mut bytes = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut bytes);
    Ok(miden_client::Word::try_from(
        bytes
            .chunks_exact(8)
            .map(|c| {
                let mut b = [0u8; 8];
                b.copy_from_slice(c);
                miden_client::Felt::new(u64::from_le_bytes(b) & 0xFFFF_FFFE_FFFF_FFFF)
                    .expect("goldilocks")
            })
            .collect::<Vec<_>>()
            .as_slice(),
    )?)
}

fn drip_script() -> Result<NoteScript, Box<dyn std::error::Error>> {
    let dusdc = AccountId::from_hex(DUSDC_FAUCET_HEX)?;
    let prefix = dusdc.prefix().as_felt().as_canonical_u64();
    let suffix = dusdc.suffix().as_canonical_u64();
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
    Ok(NoteScript::new(program))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let dispenser = AccountId::from_hex(WALLET_DISP)?;
    let payout_to = AccountId::from_hex(PAYOUT_TO)?;

    let payout_serial = rand_word()?;
    let payout_recipient = P2idNoteStorage::new(payout_to).into_recipient(payout_serial);
    let storage_felts: Vec<miden_client::Felt> =
        payout_recipient.digest().as_elements().to_vec();

    let script = drip_script()?;
    let drip_recipient = NoteRecipient::new(rand_word()?, script, NoteStorage::new(storage_felts)?);
    let assets = NoteAssets::new(vec![])?;
    let metadata = PartialNoteMetadata::new(dispenser, NoteType::Public)
        .with_tag(NoteTag::with_account_target(dispenser));
    let note = Note::new(assets, metadata, drip_recipient);
    println!("drip note id: {}", note.id());

    let home = std::env::var("HOME")?;
    let base = format!("{home}/.miden");
    let store = SqliteStore::new(PathBuf::from(format!("{base}/store.sqlite3"))).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(PathBuf::from(format!("{base}/keystore")))?
        .build()
        .await?;
    client.sync_state().await?;

    // Consume the drip note LOCALLY against the wallet dispenser.
    let req = TransactionRequestBuilder::new()
        .input_notes(vec![(note.clone(), None)])
        .build()?;
    match client.execute_transaction(dispenser, req).await {
        Ok(result) => {
            println!("✓✓ drip executed LOCALLY — MASM is correct!");
            println!("    tx: {}", result.executed_transaction().id());
        }
        Err(e) => {
            println!("✗ drip MASM runtime error:");
            println!("{e:?}");
        }
    }
    Ok(())
}
