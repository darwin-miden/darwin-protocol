//! Pure builder for a permissionless drip: given the requester + dispenser, emit
//! JSON { noteB64, payoutFileB64, payoutId }. No keys, no store, no submission —
//! the browser emits the drip note from the user's own wallet, waits for the NTX
//! builder to run it, then imports+consumes the payout file. Mirrors
//! send_confidential_deposit's --emit-json path; used by /api/drip-note.

use std::sync::Arc;

use base64::Engine as _;
use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{DefaultSourceManager, Path as AsmPath};
use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::note::{
    Note, NoteAssets, NoteRecipient, NoteScript, NoteStorage, NoteTag, NoteType,
    PartialNoteMetadata,
};
use miden_protocol::note::{NoteAttachment, NoteAttachments};
use miden_protocol::transaction::TransactionKernel;
use miden_protocol::utils::serde::Serializable;
use miden_standards::note::{NetworkAccountTarget, NoteExecutionHint, P2idNoteStorage};
use rand::RngCore;

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
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err(format!("usage: {} <requester_hex> <dispenser_hex>", args[0]).into());
    }
    let requester = AccountId::from_hex(&args[1])?;
    let dispenser = AccountId::from_hex(&args[2])?;
    let dusdc = AccountId::from_hex(DUSDC_FAUCET_HEX)?;

    // Payout recipient (P2ID to the requester) → its digest goes in the drip
    // note's storage; the drip script reads it and pays out there.
    let payout_serial = rand_word()?;
    let payout_recipient = P2idNoteStorage::new(requester).into_recipient(payout_serial);
    let storage_felts: Vec<miden_client::Felt> =
        payout_recipient.digest().as_elements().to_vec();

    // Drip request note: no asset, network-tagged, drip script + storage.
    let script = drip_script()?;
    let drip_recipient = NoteRecipient::new(rand_word()?, script, NoteStorage::new(storage_felts)?);
    let na = NetworkAccountTarget::new(dispenser, NoteExecutionHint::Always)
        .map_err(|e| format!("NetworkAccountTarget: {e:?}"))?;
    let attachments = NoteAttachments::new(vec![NoteAttachment::from(na)])
        .map_err(|e| format!("NoteAttachments: {e:?}"))?;
    let metadata = PartialNoteMetadata::new(requester, NoteType::Public)
        .with_tag(NoteTag::with_account_target(dispenser));
    let drip_note = Note::with_attachments(
        NoteAssets::new(vec![])?,
        metadata,
        drip_recipient,
        attachments,
    );

    // The private payout note the dispenser will create (for the requester to
    // import + consume). Recipient/assets match what the drip script emits.
    let payout_note = Note::new(
        NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(dusdc, DRIP_AMOUNT)?)])?,
        PartialNoteMetadata::new(dispenser, NoteType::Private),
        payout_recipient,
    );
    let payout_file = miden_protocol::note::NoteFile::NoteDetails {
        details: payout_note.clone().into(),
        after_block_num: 0u32.into(),
        tag: None,
    };

    let b64 = base64::engine::general_purpose::STANDARD;
    println!(
        "{}",
        serde_json::json!({
            "noteId": drip_note.id().to_string(),
            "noteB64": b64.encode(drip_note.to_bytes()),
            "payoutId": payout_note.id().to_string(),
            "payoutFileB64": b64.encode(payout_file.to_bytes()),
        })
    );
    Ok(())
}
