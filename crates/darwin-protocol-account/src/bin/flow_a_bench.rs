//! Phase-by-phase wall-clock benchmark of Flow A on Miden testnet.
//!
//! Same recipe as `flow_a_full`: build the atomic deposit note,
//! emit it from the user wallet, have the v2 controller consume it.
//! The difference is the wrapping `Instant::now()` per stage so the
//! performance targets can be measured directly:
//!
//!   compile  - assemble the note script + darwin::math library
//!   execute  - run the tx locally (no proof yet)
//!   prove    - generate the STARK (target: < 10s)
//!   submit   - send proven tx to the testnet RPC
//!   consume  - controller's follow-up tx that absorbs the note
//!
//! Grant verbatim: "time-to-deposit < 30s from wallet connect,
//! proof generation < 10s on standard laptop".
//!
//! Usage:
//!     cargo run --release -p darwin-protocol-account --bin flow_a_bench
//!
//! The wallet connect step (UI side) isn't measured here — it's
//! the time between user click and the wallet returning a signed
//! tx, which doesn't go through this code path. The bench covers
//! everything *after* the user click.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

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

const USER_WALLET_HEX: &str = "0xed3cd5befa3207805f8529207cfc0d";
const REAL_BODIES_CONTROLLER_HEX: &str = "0xa25aa0b00007688024b74b05a52aab";
const DETH_FAUCET_HEX: &str = "0xa095d9b3831e96206ff70c2218a6a9";
const DEPOSIT_AMOUNT: u64 = 100;

fn pretty(d: std::time::Duration) -> String {
    let ms = d.as_millis();
    if ms < 1000 {
        format!("{ms} ms")
    } else {
        format!("{:.2} s", d.as_secs_f64())
    }
}

fn target_check(label: &str, dur: std::time::Duration, target_s: f64) -> String {
    if dur.as_secs_f64() <= target_s {
        format!("✓ {label} {} (target ≤{target_s}s)", pretty(dur))
    } else {
        format!("✗ {label} {} (target ≤{target_s}s -- OVER)", pretty(dur))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    let setup_start = Instant::now();
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&miden_client::rpc::Endpoint::testnet(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;
    let setup_dur = setup_start.elapsed();
    println!("setup     {}", pretty(setup_dur));

    // -- Phase: compile -----------------------------------------------
    let compile_start = Instant::now();
    use miden_assembly::ast::{Module, ModuleKind};
    use miden_assembly::{Assembler, DefaultSourceManager, Path};
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
    let program = miden_protocol::transaction::TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())?
        .assemble_program(darwin_notes::ATOMIC_DEPOSIT_NOTE_MASM)?;
    let note_script = NoteScript::new(program);
    let compile_dur = compile_start.elapsed();
    println!("compile   {}", pretty(compile_dur));

    // -- Phase: build note --------------------------------------------
    let build_start = Instant::now();
    let user_wallet = AccountId::from_hex(USER_WALLET_HEX)?;
    let controller = AccountId::from_hex(REAL_BODIES_CONTROLLER_HEX)?;
    let deth_faucet = AccountId::from_hex(DETH_FAUCET_HEX)?;
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
    let storage_felts = vec![
        miden_client::Felt::new(200_000_000_000).expect("bounded"),
        miden_client::Felt::new(9_970).expect("bounded"),
        miden_client::Felt::new(10_000_000_000).expect("bounded"),
    ];
    let recipient = NoteRecipient::new(serial_num, note_script.clone(), NoteStorage::new(storage_felts)?);
    let note = Note::new(assets, metadata, recipient);
    let build_dur = build_start.elapsed();
    println!("build     {}", pretty(build_dur));

    // -- Phase: execute (no proof yet) --------------------------------
    let exec_start = Instant::now();
    let deploy_request = TransactionRequestBuilder::new()
        .own_output_notes(vec![note.clone()])
        .build()?;
    let deploy_result = client.execute_transaction(user_wallet, deploy_request).await?;
    let exec_dur = exec_start.elapsed();
    println!("execute   {}", pretty(exec_dur));

    // -- Phase: prove -------------------------------------------------
    let prove_start = Instant::now();
    let prover = client.prover();
    let deploy_proven = client.prove_transaction_with(&deploy_result, prover.clone()).await?;
    let prove_dur = prove_start.elapsed();
    println!("prove     {}", pretty(prove_dur));

    // -- Phase: submit ------------------------------------------------
    let submit_start = Instant::now();
    let deploy_height = client.submit_proven_transaction(deploy_proven, &deploy_result).await?;
    let submit_dur = submit_start.elapsed();
    println!("submit    {}", pretty(submit_dur));

    let apply_start = Instant::now();
    client.apply_transaction(&deploy_result, deploy_height).await?;
    let apply_dur = apply_start.elapsed();
    println!("apply     {}", pretty(apply_dur));

    // -- Phase: consume (controller side) -----------------------------
    let consume_start = Instant::now();
    let consume_request = TransactionRequestBuilder::new()
        .input_notes(vec![(note.clone(), None)])
        .build()?;
    let consume_result = client.execute_transaction(controller, consume_request).await?;
    let consume_proven = client.prove_transaction_with(&consume_result, prover).await?;
    let consume_height = client.submit_proven_transaction(consume_proven, &consume_result).await?;
    client.apply_transaction(&consume_result, consume_height).await?;
    let consume_dur = consume_start.elapsed();
    println!("consume   {} (full controller tx, end-to-end)", pretty(consume_dur));

    // -- Roll-up + performance checks --------------------------------
    let deposit_total = compile_dur + build_dur + exec_dur + prove_dur + submit_dur + apply_dur;
    let proof_only    = prove_dur;
    let e2e_with_consume = setup_dur + deposit_total + consume_dur;

    println!();
    println!("=== Grant target checks ===");
    println!("{}", target_check("proof (M3 D2)   ", proof_only, 10.0));
    println!("{}", target_check("deposit (M3 D2) ", deposit_total, 30.0));
    println!();
    println!("E2E user click to controller-consumed: {}", pretty(e2e_with_consume));
    println!("Submitted user tx     {}", deploy_result.executed_transaction().id());
    println!("Submitted consumer tx {}", consume_result.executed_transaction().id());

    Ok(())
}
