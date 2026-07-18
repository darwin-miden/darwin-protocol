//! Pure builder for a permissionless drip: given the requester + dispenser, emit
//! JSON { noteId, noteB64 }. No keys, no store, no submission — the browser emits
//! this drip note from the user's own wallet; the network's NTX builder runs it
//! against the dispenser, which creates a PUBLIC P2ID payout note TAGGED for the
//! requester. The requester's wallet (MidenFi) then discovers that payout on sync
//! and consumes it like any standard payment — so, unlike the old private-payout
//! design, the builder does NOT need to hand back any payout note to consume.
//! Used by /api/drip-note.

use base64::Engine as _;
use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::note::{
    Note, NoteAssets, NoteRecipient, NoteStorage, NoteTag, NoteType, PartialNoteMetadata,
};
use miden_protocol::note::{NoteAttachment, NoteAttachments};
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

/// Parse an account id from either hex (0x…) or bech32 (mtst1…) — MidenFi hands
/// the browser a bech32 address, while the CLI/derived-wallet paths use hex.
fn parse_account(s: &str) -> Result<AccountId, Box<dyn std::error::Error>> {
    if let Ok(id) = AccountId::from_hex(s) {
        return Ok(id);
    }
    match AccountId::from_bech32(s) {
        Ok((_net, id)) => Ok(id),
        Err(e) => Err(format!("requester '{s}' is neither hex nor bech32: {e:?}").into()),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 3 {
        return Err(format!("usage: {} <requester> <dispenser>", args[0]).into());
    }
    let requester = parse_account(&args[1])?;
    let dispenser = parse_account(&args[2])?;
    let dusdc = AccountId::from_hex(DUSDC_FAUCET_HEX)?;

    // Drip note storage the script reads (elements 0..5):
    //   [0] target_suffix, [1] target_prefix, [2..5] SERIAL_NUM.
    // The SERIAL is random so each payout note the dispenser creates has a fresh
    // id (two requests from the same account don't collide).
    let serial = rand_word()?;
    let mut storage_felts = vec![requester.suffix(), requester.prefix().as_felt()];
    storage_felts.extend_from_slice(serial.as_elements());

    let script = darwin_protocol_account::drip_note_script(
        dusdc.prefix().as_felt().as_canonical_u64(),
        dusdc.suffix().as_canonical_u64(),
        DRIP_AMOUNT,
    )?;

    // Drip request note: no asset, network-tagged for the dispenser, drip script
    // + storage. The NTX builder consumes it against the dispenser.
    let drip_recipient =
        NoteRecipient::new(rand_word()?, script, NoteStorage::new(storage_felts)?);
    let na = NetworkAccountTarget::new(dispenser, NoteExecutionHint::Always)
        .map_err(|e| format!("NetworkAccountTarget: {e:?}"))?;
    let attachments = NoteAttachments::new(vec![NoteAttachment::from(na)])
        .map_err(|e| format!("NoteAttachments: {e:?}"))?;
    let metadata = PartialNoteMetadata::new(requester, NoteType::Public)
        .with_tag(NoteTag::with_account_target(dispenser));
    let drip_note =
        Note::with_attachments(NoteAssets::new(vec![])?, metadata, drip_recipient, attachments);

    // Compute the id of the PUBLIC P2ID payout the drip will create. NoteId =
    // hash(recipient, assets) — independent of metadata — and the drip's
    // p2id::new builds the recipient from the SAME target + serial + standard
    // P2ID script, so this id matches the on-chain payout (verified via
    // debug_drip's expected_output_recipients). Returning it lets the browser
    // consume by id directly (no extra "read consumable notes" wallet prompt).
    let payout_recipient = P2idNoteStorage::new(requester).into_recipient(serial);
    let payout_note = Note::new(
        NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(dusdc, DRIP_AMOUNT)?)])?,
        PartialNoteMetadata::new(dispenser, NoteType::Public)
            .with_tag(NoteTag::with_account_target(requester)),
        payout_recipient,
    );

    let b64 = base64::engine::general_purpose::STANDARD;
    println!(
        "{}",
        serde_json::json!({
            "noteId": drip_note.id().to_string(),
            "noteB64": b64.encode(drip_note.to_bytes()),
            "payoutId": payout_note.id().to_string(),
        })
    );
    Ok(())
}
