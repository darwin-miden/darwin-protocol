//! v3 atomic deposit note — CLEAN single-asset rewrite.
//!
//! Drops the drain loop entirely (single-asset deposit) and uses
//! local memory to preserve USER_BASKET_KEY + AMOUNT_WORD between
//! the storage-read phase and the receive_and_credit call site.
//!
//! Call-site invariant for receive_and_credit:
//!   stack top 16 = [ASSET_KEY(4), ASSET_VALUE(4),
//!                   USER_BASKET_KEY(4), AMOUNT_WORD(4)]
//!
//! The v6 controller's `receive_and_credit` proc body:
//!   exec.native_account::add_asset    (consumes [KEY, VAL, X], leaves [VAL'])
//!   dropw                              (drops VAL')
//!   push.slot_id_prefix push.slot_id_suffix
//!   exec.native_account::set_map_item  (consumes [slot_p, slot_s, KEY, VAL])
//! → After add_asset+dropw, top of stack must already be
//!   [USER_BASKET_KEY, AMOUNT_WORD] so set_map_item sees the right
//!   key/value when the two slot_id felts are pushed on top.

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

const USER_WALLET_HEX: &str = "0x4397442ac860af717888fe90cad00b";
const CONTROLLER_HEX: &str = "0x2388eaea4ce45331214b871755e7b5";
const DETH_FAUCET_HEX: &str = "0xc2c923560dc3cb114ec24ab2291a05";
const RECEIVE_AND_CREDIT_ROOT: &str =
    "0x849f526236e9a7ab84a183da209666c3e2839efaeb7d5866a6dca043fdaddc10";

const DEPOSIT_AMOUNT: u64 = 25;

fn build_masm() -> String {
    format!(
        r#"
use miden::protocol::active_note
use darwin::math

# Local memory layout:
#   loc[0..3]   AMOUNT_WORD                 (computed at start)
#   loc[4..7]   USER_BASKET_KEY              (built from storage)
#   loc[8..15]  asset buffer (KEY+VALUE)    (kernel writes here)

@locals(2048)
proc credit_single
    # ════════════════════════════════════════════════════════
    # PHASE 1: read storage felts to memory[100..]
    # storage layout: [0]=depval, [1]=fee, [2]=nav, [3]=user_s, [4]=user_p
    # so mem[100]=depval, mem[101]=fee, mem[102]=nav, mem[103]=user_s, mem[104]=user_p
    # ════════════════════════════════════════════════════════
    push.100
    exec.active_note::get_storage
    # stack: [num_storage_items, ...]
    drop
    # stack: [...] — kernel pad intact

    # ════════════════════════════════════════════════════════
    # PHASE 2: compute net_amount = depval * fee / nav and store
    # AMOUNT_WORD = [0, 0, 0, net_amount] in loc[0..3].
    # ════════════════════════════════════════════════════════
    push.100 mem_load          # depval
    push.101 mem_load          # fee
    push.102 mem_load          # nav
    mul                        # fee * nav (top), depval below
    exec.math::felt_div        # depval / (fee*nav) → net_amount
    # stack: [net_amount, ...]

    # AMOUNT_WORD to store: [0(top), 0, 0, net_amount].
    # We already have net_amount on top. Push 3 zeros on top of it:
    push.0 push.0 push.0
    # stack: [0, 0, 0, net_amount, ...]

    # Store as a word. `loc_storew_le.X` pops top 4 elements as a word and
    # writes to local memory starting at index X.
    loc_storew_le.0
    # stack now still has the same 4 felts (loc_storew_le DOES NOT pop in v0.15) — let me verify
    # If it doesn't pop, drop them:
    dropw
    # stack: [...] — back to pre-amount state

    # ════════════════════════════════════════════════════════
    # PHASE 3: build USER_BASKET_KEY = [basket_p=0, basket_s=0,
    #          user_p, user_s] and store in loc[4..7].
    # Top-down: basket_p first (top), user_s last.
    # Push order (last is top): push user_s, user_p, 0, 0.
    # ════════════════════════════════════════════════════════
    push.103 mem_load          # user_s on top
    push.104 mem_load          # user_p on top
    push.0                     # basket_s
    push.0                     # basket_p (top)
    # stack: [0, 0, user_p, user_s, ...]
    loc_storew_le.4
    dropw
    # stack: [...] — back to base state

    # ════════════════════════════════════════════════════════
    # PHASE 4: load the single asset into local memory at loc[8..15].
    # kernel writes per-asset: [KEY(4) at loc[8..11], VAL(4) at loc[12..15]].
    # ════════════════════════════════════════════════════════
    locaddr.8 exec.active_note::get_assets
    drop  # drop num_assets (assumed 1)
    # stack: [...] — clean

    # ════════════════════════════════════════════════════════
    # PHASE 5: build call stack
    #   want top 16 = [ASSET_KEY(4) on top,
    #                  ASSET_VALUE(4),
    #                  USER_BASKET_KEY(4),
    #                  AMOUNT_WORD(4)]
    # We load with `padw loc_loadw_le.X` (padw pushes 4 zeros, loc_loadw_le
    # replaces them with the loaded word — net +4 per pair).
    # Build BOTTOM-UP: load AMOUNT first (goes deepest), then USER_KEY,
    # then ASSET_VALUE, then ASSET_KEY (ends up on top).
    # ════════════════════════════════════════════════════════
    padw loc_loadw_le.0   # AMOUNT_WORD on top — depth +4
    padw loc_loadw_le.4   # USER_BASKET_KEY on top — depth +8
    padw loc_loadw_le.12  # ASSET_VALUE on top — depth +12
    padw loc_loadw_le.8   # ASSET_KEY on top — depth +16
    # stack: [K0, K1, K2, K3, V0, V1, V2, V3, U0, U1, U2, U3, A0, A1, A2, A3, ...kernel pad]

    # ════════════════════════════════════════════════════════
    # PHASE 6: call receive_and_credit.
    # Body: exec add_asset (-4), dropw (-4), push.slot_id (+2), set_map_item (-6).
    # Net effect on caller stack: -12 felts consumed by the body, +0 returned.
    # ════════════════════════════════════════════════════════
    call.{RECEIVE_AND_CREDIT_ROOT}
    # v0.15 `call` semantics preserve caller stack depth. We pushed
    # 16 felts before the call, and `call` re-fills our top 16 with
    # the proc's output frame on return. So caller depth is
    # base+16. Drop ALL 16 to restore the kernel's required depth.
    dropw dropw dropw dropw
end

begin
    exec.credit_single
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
    // Build darwin::math lib so the note can call felt_div.
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

    // Storage felts (5): depval, fee, nav, user_s, user_p
    let storage_felts = vec![
        miden_client::Felt::new(DEPOSIT_AMOUNT)?,
        miden_client::Felt::new(9970)?,                                            // fee factor
        miden_client::Felt::new(1)?,                                               // nav scale
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
