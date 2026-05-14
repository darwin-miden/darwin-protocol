//! Consume the atomic Flow A DepositNote from the controller side.
//!
//! Pair to `deploy_atomic_flow_a.rs`. That binary published a note
//! from the user wallet carrying 100 dETH to the real-bodies
//! controller. This binary signs and submits a transaction *from the
//! controller* that consumes the note, so the dETH lands in the
//! controller's vault and the atomic-deposit MASM runs on-chain.
//!
//! The two binaries together complete Flow A end-to-end on Miden
//! testnet:
//!
//!   User wallet → (deploy_atomic_flow_a) → atomic deposit note
//!                                          ↓
//!   Darwin controller ← (consume_atomic_flow_a) ← consumes note,
//!     runs darwin::math::felt_div via miden::core::math::u64::div,
//!     dETH moves into the controller's vault.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin consume_atomic_flow_a -- \
//!         --note-id 0x979bfdbb…6a65bd1f
//!
//! If --note-id is omitted, falls back to the one recorded in
//! darwin-baskets/state/testnet.toml as the last atomic note.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::note::NoteId;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;

const REAL_BODIES_CONTROLLER_HEX: &str = "0x171f46fecf1bca8005ae068a8dfe77";
const DEFAULT_NOTE_ID: &str = "0x979bfdbb7f532dc27c582f2cd694a8ea7a2b92da665b54785f94359b6a65bd1f";

fn parse_args() -> String {
    let mut note_id: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--note-id" => note_id = Some(args.next().expect("--note-id needs a value")),
            other => panic!("unknown flag {other}"),
        }
    }
    note_id.unwrap_or_else(|| DEFAULT_NOTE_ID.to_string())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let note_id_hex = parse_args();
    let note_id = NoteId::try_from_hex(&note_id_hex)?;

    let home = std::env::var("HOME").expect("HOME set");
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    println!("Setting up miden-client against testnet…");
    let store = SqliteStore::new(store_path.clone()).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    let controller = AccountId::from_hex(REAL_BODIES_CONTROLLER_HEX)?;
    println!("Target controller: {controller}");
    println!("Note to consume:   {note_id}");

    // Sync is intentionally skipped: the local sqlite store has
    // accumulated state from earlier sessions and the testnet node
    // is currently returning a wire-format mismatch that crashes
    // miden-client's sync invariants. The note we want is the one
    // we just created via `deploy_atomic_flow_a`, which is already
    // in the local input-note store from that same session's
    // `apply_transaction` call.
    println!("(skipping sync — using local input-note store directly)");

    // Look up the note in the input-note store. The note was created
    // by the user-side tx and should be discoverable as a public note.
    let note_record = client
        .get_input_note(note_id)
        .await?
        .ok_or("note not found in the local input-note store — make sure the user-side tx has been observed on-chain")?;
    println!("Found note in the store.");
    use miden_client::note::Note;
    let note: Note = TryInto::<Note>::try_into(note_record)
        .map_err(|e| format!("note record could not be materialised: {e}"))?;

    // Build a TransactionRequest that consumes this note. The
    // controller signs the tx (its key is in the local keystore).
    let tx_request = TransactionRequestBuilder::new()
        .build_consume_notes(vec![note])?;

    println!("Executing transaction (controller-side consumption)…");
    let tx_result = client.execute_transaction(controller, tx_request).await?;
    let executed = tx_result.executed_transaction().clone();
    println!("Executed. tx id: {}", executed.id());

    println!("Proving transaction…");
    let prover = client.prover();
    let proven = client.prove_transaction_with(&tx_result, prover).await?;

    println!("Submitting…");
    let height = client.submit_proven_transaction(proven, &tx_result).await?;
    println!("Submitted at block height: {height}");

    println!("Applying locally…");
    client.apply_transaction(&tx_result, height).await?;

    println!();
    println!("🎯 Flow A closed end-to-end on Miden testnet.");
    println!("   note consumed:  {note_id}");
    println!("   consumer tx id: {}", executed.id());
    println!("   block height:   {height}");

    Ok(())
}
