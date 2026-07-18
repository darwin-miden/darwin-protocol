//! Debug the drip note by consuming it LOCALLY against the wallet dispenser
//! (a regular account with a key + BasicWallet + dUSDC in its vault). Running
//! it locally surfaces the MASM runtime error directly — the NTX builder path
//! is opaque. Once this succeeds locally, the network path will too.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{
    Note, NoteAssets, NoteRecipient, NoteStorage, NoteTag, NoteType, PartialNoteMetadata,
};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let dispenser = AccountId::from_hex(WALLET_DISP)?;
    let payout_to = AccountId::from_hex(PAYOUT_TO)?;

    // Storage the drip script reads: [target_suffix, target_prefix, SERIAL(4)].
    let serial = rand_word()?;
    let mut storage_felts = vec![payout_to.suffix(), payout_to.prefix().as_felt()];
    storage_felts.extend_from_slice(serial.as_elements());

    // The PUBLIC P2ID payout the MASM will create. Providing its recipient makes
    // the executor register the P2ID script (a public note records its script
    // on-chain) AND validates our recipient matches the MASM's p2id::new output.
    let payout_recipient = P2idNoteStorage::new(payout_to).into_recipient(serial);

    // The payout NoteId we'd compute in build_drip_note (recipient + 5 dUSDC).
    // Must equal the output note the MASM creates below.
    let dusdc_for_id = AccountId::from_hex(DUSDC_FAUCET_HEX)?;
    let expected_payout = Note::new(
        NoteAssets::new(vec![miden_client::asset::Asset::Fungible(
            miden_client::asset::FungibleAsset::new(dusdc_for_id, DRIP_AMOUNT)?,
        )])?,
        PartialNoteMetadata::new(dispenser, NoteType::Public)
            .with_tag(NoteTag::with_account_target(payout_to)),
        payout_recipient.clone(),
    );
    println!("computed payout id (build_drip_note): {}", expected_payout.id());

    let dusdc = AccountId::from_hex(DUSDC_FAUCET_HEX)?;
    let script = darwin_protocol_account::drip_note_script(
        dusdc.prefix().as_felt().as_canonical_u64(),
        dusdc.suffix().as_canonical_u64(),
        DRIP_AMOUNT,
    )?;
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
        .expected_output_recipients(vec![payout_recipient])
        .build()?;
    match client.execute_transaction(dispenser, req).await {
        Ok(result) => {
            println!("✓✓ drip executed LOCALLY — MASM is correct!");
            println!("    tx: {}", result.executed_transaction().id());
            println!("    payout target should be: {payout_to}");
            let out = result.executed_transaction().output_notes();
            println!("    created {} output note(s):", out.num_notes());
            for on in out.iter() {
                println!(
                    "      id={} type={:?} recipient={:?}",
                    on.id(),
                    on.metadata().note_type(),
                    on.recipient_digest(),
                );
            }
        }
        Err(e) => {
            println!("✗ drip MASM runtime error:");
            println!("{e:?}");
        }
    }
    Ok(())
}
