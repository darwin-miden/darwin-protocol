//! Skip the asset-memory roundtrip entirely. Push the FungibleAsset
//! KEY+VALUE words on the stack as constants and call receive_asset
//! directly. Isolates whether the kernel rejects the asset bytes or
//! the MASM loads them wrong from memory.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::asset::{Asset, FungibleAsset};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::{
    Note, NoteAssets, NoteRecipient, NoteScript, NoteStorage, NoteType,
};
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use rand::RngCore;

const USER_WALLET_HEX: &str = "0x4397442ac860af717888fe90cad00b";
const CONTROLLER_HEX: &str = "0x2388eaea4ce45331214b871755e7b5";
const DETH_FAUCET_HEX: &str = "0xc2c923560dc3cb114ec24ab2291a05";
const RECEIVE_ASSET_ROOT: &str =
    "0x6170fd6d682d91777b551fd866258f43cc657f1291f8f071500f4e56e9c153da";
const DEPOSIT_AMOUNT: u64 = 50;

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

    let user_wallet = AccountId::from_hex(USER_WALLET_HEX)?;
    let controller = AccountId::from_hex(CONTROLLER_HEX)?;
    let deth_faucet = AccountId::from_hex(DETH_FAUCET_HEX)?;
    let fa = FungibleAsset::new(deth_faucet, DEPOSIT_AMOUNT)?;
    let key = fa.to_key_word();
    let val = fa.to_value_word();

    // Build MASM that hardcodes [KEY, VAL, pad(8)] on stack.
    // MASM push order: the LAST push lands on top.
    // We want stack TOP = KEY[0] (asset_id.suffix). So push in
    // reverse: pad(8) first, then VAL[3..0], then KEY[3..0].
    // Kernel sets up note exec stack at depth 16 (16 zeros) on
    // entry. Push KEY+VAL (8 felts) on top → depth 24. Call
    // receive_asset, which is depth-neutral under v0.15 call
    // semantics. Then drop the 8 felts we added so depth returns
    // to 16 for the kernel's final check.
    let masm = format!(
        r#"
begin
    # VAL word — pushed reversed so VAL[0] ends up at depth 4
    push.{v3} push.{v2} push.{v1} push.{v0}
    # KEY word — pushed reversed so KEY[0] ends up on top
    push.{k3} push.{k2} push.{k1} push.{k0}
    # stack: [KEY (top=K0), VAL, pad(16 kernel default)]
    call.{RECEIVE_ASSET_ROOT}
    # call preserves caller depth; drop the 8 we added.
    dropw dropw
end
"#,
        v3 = val[3].as_canonical_u64(),
        v2 = val[2].as_canonical_u64(),
        v1 = val[1].as_canonical_u64(),
        v0 = val[0].as_canonical_u64(),
        k3 = key[3].as_canonical_u64(),
        k2 = key[2].as_canonical_u64(),
        k1 = key[1].as_canonical_u64(),
        k0 = key[0].as_canonical_u64(),
    );
    println!("MASM:\n{masm}");

    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .assemble_program(masm.as_str())?;
    let note_script = NoteScript::new(program);

    let assets = NoteAssets::new(vec![Asset::Fungible(fa)])?;
    let metadata =
        miden_protocol::note::PartialNoteMetadata::new(user_wallet, NoteType::Public);
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
    let recipient = NoteRecipient::new(serial_num, note_script, NoteStorage::new(vec![])?);
    let note = Note::new(assets, metadata, recipient);
    println!("Note id: {}", note.id());

    println!("=== emit ===");
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

    println!("=== consume by controller ===");
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
