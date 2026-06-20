//! v3 single-asset path — bypasses the drain loop entirely.
//!
//! The drain loop sets up [ASSET_KEY, ASSET_VALUE, pad(8)] which is
//! correct for receive_asset but wrong for receive_and_credit (the
//! latter needs the user_basket_key + amount_word right after the
//! ASSET_KEY/VAL pair, but the loop padding buries them).
//!
//! This note script hard-codes the deposit being exactly ONE asset
//! and sets up the stack inline as
//! [ASSET_KEY, ASSET_VALUE, USER_BASKET_KEY, AMOUNT_WORD] so
//! receive_and_credit's set_map_item finds the right
//! USER_BASKET_KEY+AMOUNT_WORD on top after the add_asset+dropw.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{Note, NoteAssets, NoteRecipient, NoteScript, NoteStorage, NoteType};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use rand::RngCore;

const USER_WALLET_HEX: &str = "0x4397442ac860af717888fe90cad00b";
const CONTROLLER_HEX: &str = "0x2388eaea4ce45331214b871755e7b5";
const DETH_FAUCET_HEX: &str = "0xc2c923560dc3cb114ec24ab2291a05";
const RECEIVE_AND_CREDIT_ROOT: &str =
    "0x849f526236e9a7ab84a183da209666c3e2839efaeb7d5866a6dca043fdaddc10";

const DEPOSIT_AMOUNT: u64 = 30;

/// Build a single-asset receive_and_credit note. NO drain loop.
/// Storage at runtime: [user_id_suffix, user_id_prefix]
/// The asset key+value are loaded from memory (the kernel writes the
/// note's vault assets at locaddr.0 via active_note::get_assets).
fn build_masm() -> String {
    format!(
        r#"
use miden::protocol::active_note
use miden::protocol::asset::ASSET_VALUE_MEMORY_OFFSET

@locals(2048)
proc credit_one
    # 1. Read user_id_{{suffix,prefix}} from storage memory[100,101].
    push.100
    exec.active_note::get_storage
    drop drop                       # drop num_items + a kernel pad felt
    # Memory now has storage at [100..]:
    #   [100] = user_id_suffix
    #   [101] = user_id_prefix

    # 2. Load the single asset into local memory at locaddr.0.
    locaddr.0 exec.active_note::get_assets
    drop                            # drop num_assets (we assume exactly 1)

    # 3. Load AMOUNT_WORD (the asset's value word) onto stack as
    #    a placeholder for receive_and_credit's `VALUE` arg. We use
    #    the asset's amount directly as the position amount.
    padw                            # pad for mem_loadw target
    locaddr.0 add.ASSET_VALUE_MEMORY_OFFSET
    mem_loadw_le
    # Stack: [VALUE_word (4 felts), ...]

    # 4. Push USER_BASKET_KEY = [basket_prefix=0, basket_suffix=0,
    #                            user_id_prefix, user_id_suffix].
    push.101 mem_load               # user_id_prefix
    push.100 mem_load               # user_id_suffix
    push.0 push.0
    # Stack: [0, 0, user_id_prefix, user_id_suffix, VALUE_word, ...]
    # Wait we need USER_BASKET_KEY ordered so [0, 0, user_p, user_s] is
    # the key on top, then VALUE_word below.

    # 5. Load ASSET_VALUE word.
    padw
    locaddr.0 add.ASSET_VALUE_MEMORY_OFFSET
    mem_loadw_le
    # Stack: [ASSET_VALUE_word, USER_BASKET_KEY, VALUE_word, ...]

    # 6. Load ASSET_KEY word.
    padw locaddr.0 mem_loadw_le
    # Stack: [ASSET_KEY, ASSET_VALUE, USER_BASKET_KEY, VALUE_word, ...]

    # 7. Pad for the call (need [KEY, VAL, USER_KEY, AMT_VAL, pad]).
    # We already have 16 felts in the right order — top 16:
    #   [ASSET_KEY (4), ASSET_VALUE (4), USER_BASKET_KEY (4), AMOUNT_WORD (4)]

    call.{RECEIVE_AND_CREDIT_ROOT}
    # receive_and_credit:
    #   exec add_asset -> consumes [KEY, VAL, pad(7)], leaves [VAL'] + cleanup
    #   dropw         -> stack now [USER_BASKET_KEY, AMOUNT_WORD, ...]
    #   push slot_id_prefix, slot_id_suffix
    #   exec set_map_item -> consumes 10 (slot_id_p, slot_id_s, KEY=USER_BASKET_KEY, VAL=AMOUNT_WORD)
    # Net effect: vault credited + slot 10 user_position written.

    # 8. Clean any remaining felts to depth 16.
    dropw dropw
end

begin
    exec.credit_one
end
"#
    )
}

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

    let masm = build_masm();
    println!("MASM:\n{masm}");
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .assemble_program(masm.as_str())?;
    let note_script = NoteScript::new(program);

    let user_wallet = AccountId::from_hex(USER_WALLET_HEX)?;
    let controller = AccountId::from_hex(CONTROLLER_HEX)?;
    let deth_faucet = AccountId::from_hex(DETH_FAUCET_HEX)?;

    let assets = NoteAssets::new(vec![Asset::Fungible(FungibleAsset::new(
        deth_faucet,
        DEPOSIT_AMOUNT,
    )?)])?;
    let metadata =
        miden_protocol::note::PartialNoteMetadata::new(user_wallet, NoteType::Public);

    let storage_felts = vec![
        miden_client::Felt::new(user_wallet.suffix().as_canonical_u64())?,
        miden_client::Felt::new(user_wallet.prefix().as_felt().as_canonical_u64())?,
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
    println!("Note id: {}", note.id());

    println!();
    println!("=== Step 1: emit ===");
    let r = client
        .execute_transaction(
            user_wallet,
            TransactionRequestBuilder::new()
                .own_output_notes(vec![note.clone()])
                .build()?,
        )
        .await?;
    let prover = client.prover();
    let p = client.prove_transaction_with(&r, prover.clone()).await?;
    let h = client.submit_proven_transaction(p, &r).await?;
    client.apply_transaction(&r, h).await?;
    println!("emit @ {h}");

    println!();
    println!("=== Step 2: controller consume (receive_and_credit) ===");
    let r = client
        .execute_transaction(
            controller,
            TransactionRequestBuilder::new()
                .input_notes(vec![(note.clone(), None)])
                .build()?,
        )
        .await?;
    let p = client.prove_transaction_with(&r, prover).await?;
    let h = client.submit_proven_transaction(p, &r).await?;
    client.apply_transaction(&r, h).await?;
    println!("✅ consume @ {h}");
    Ok(())
}
