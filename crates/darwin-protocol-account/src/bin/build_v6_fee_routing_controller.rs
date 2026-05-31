//! Build a `.masp` for the v6 Darwin Protocol Account controller.
//!
//! v6 = v5 + the two pieces TODO 3 (mint_to_user atomic chain) and
//! TODO 4 (fee routing) from prior audit cycles.
//!
//! Specifically:
//!
//!   1. Slot 11 (fee_recipient_account_id) — the canonical destination
//!      for protocol fees (mint, redeem, mgmt). Admin proc
//!      `set_fee_recipient(account_id_word)`. Public read
//!      `get_fee_recipient()`.
//!
//!   2. Compound proc `receive_and_credit(user_basket_key_word,
//!      basket_amount_word)` — atomically calls receive_asset (legacy)
//!      and writes slot 10 (user position) in one call site, so a
//!      consuming note only needs ONE `call.X` to land the asset + the
//!      ledger row.
//!
//! v6 stays a strict superset of v5: every prior MAST root is
//! preserved (compute_*, accrue_*, receive_asset, read_pool_position,
//! execute_rebalance_step, get_target_weights, get_fees,
//! get_user_position, set_target_weights, set_fees, set_user_position).
//! Deploying v6 to testnet is a one-line `miden client new-account`
//! followed by a fresh `deploy_v6_init` admin tx.
//!
//! Why not deploy v6 immediately: v5 was deployed today + initialised
//! with basket configs. Redeploying invalidates the init tx and the
//! atomic_deposit_note_v2 MAST root reference. v6 lands when we're
//! ready to wire fee routing + the compound credit proc into the
//! atomic notes — that's the next session's a future iteration work.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account \
//!         --bin build_v6_fee_routing_controller -- \
//!         --out /tmp/darwin-v6.masp

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::serde::Serializable;
use miden_assembly::{DefaultSourceManager, Path};
use miden_mast_package::{Package, PackageId, Section, SectionId, TargetType, Version};
use miden_protocol::account::component::AccountComponentMetadata;
use miden_protocol::account::AccountType;

const CONTROLLER_NAMESPACE: &str = "darwin::controller";
const MATH_NAMESPACE: &str = "darwin::math";

// Slot 11 (fee_recipient) ID — same derivation pattern as v5 slots
// (`hash_string_to_word("darwin::slot_11")[0..2]`). Run
// `compute_slot_ids` and append the slot 11 row when v6 lands.
// compute_slot_ids → slot 11 "darwin::slot_11"
const SLOT_11_SUFFIX: u64 = 7136484239511356554;
const SLOT_11_PREFIX: u64 = 15534174776004786237;

const V6_CONTROLLER_SOURCE: &str = r#"
use darwin::math
use miden::protocol::native_account
use miden::protocol::active_account

# ---------------------------------------------------------------------
# Storage layout — v6 = v5 + slot 11
#   0  VERSION
#   1  BASKET_FAUCET_ID
#   2  pool_positions           map[faucet_id_word → u64]
#   3  target_weights           map[basket_id_word → bps_word]
#   4  fees                     map[basket_id_word → bps_word]
#  10  user_positions           map[(user_id ‖ basket_id) → amt_word]
#  11  fee_recipient            value(account_id_word)             ← NEW
# ---------------------------------------------------------------------

# v2-compatible procs preserved.
pub proc compute_nav        exec.math::felt_div end
pub proc apply_deposit      add end
pub proc apply_redeem       sub end
pub proc compute_mint_amount    exec.math::felt_div end
pub proc compute_redeem_amount  exec.math::felt_div end
pub proc accrue_management_fee  mul end
pub proc receive_asset
    exec.native_account::add_asset
    dropw
end

# v3-compatible.
pub proc read_pool_position
    push.4481777022490664135 push.811430137917007933
    exec.active_account::get_map_item
end

# v4-compatible.
pub proc execute_rebalance_step  drop drop end

# v5: basket config.
pub proc get_target_weights
    push.12444993101681295303 push.6486922254117069551
    exec.active_account::get_map_item
end
pub proc get_fees
    push.10941162321188629145 push.16076714866331093212
    exec.active_account::get_map_item
end
pub proc set_target_weights
    push.12444993101681295303 push.6486922254117069551
    exec.native_account::set_map_item
end
pub proc set_fees
    push.10941162321188629145 push.16076714866331093212
    exec.native_account::set_map_item
end

# v5: per-user positions.
pub proc get_user_position
    push.14059285908597291169 push.15366932551269667247
    exec.active_account::get_map_item
end
pub proc set_user_position
    push.14059285908597291169 push.15366932551269667247
    exec.native_account::set_map_item
end

# ---------------------------------------------------------------------
# v6 — fee recipient (TODO 4)
# ---------------------------------------------------------------------

#! Read the fee_recipient account id word from slot 11.
#! Stack on exit: [account_id_word(4)]
pub proc get_fee_recipient
    push.SLOT_11_PREFIX_FELT push.SLOT_11_SUFFIX_FELT
    exec.active_account::get_item
end

#! Admin: write the fee_recipient account id word into slot 11.
#! Stack on entry: [account_id_word(4), pad(12)]
pub proc set_fee_recipient
    push.SLOT_11_PREFIX_FELT push.SLOT_11_SUFFIX_FELT
    exec.native_account::set_item
end

# ---------------------------------------------------------------------
# v6 — compound credit proc (TODO 3)
#
# `receive_and_credit` does in one call site what the v2 atomic note
# does in two:
#   1. receive_asset (assets land in controller vault)
#   2. set_user_position (slot 10 updated)
#
# Stack on entry:
#   [asset_word(4), user_basket_key_word(4), basket_amount_word(4), pad(4)]
# Stack on exit:
#   [old_user_position_word(4), pad]
#
# Future atomic_deposit_note_v3 collapses to a single `call.X` into
# this proc — fewer kernel transitions, cheaper tx, easier audit.
# ---------------------------------------------------------------------
pub proc receive_and_credit
    exec.native_account::add_asset
    dropw
    push.14059285908597291169 push.15366932551269667247
    exec.native_account::set_map_item
end
"#;

fn parse_args() -> PathBuf {
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--out" || a == "-o" {
            out = Some(PathBuf::from(args.next().expect("--out value")));
        }
    }
    out.unwrap_or_else(|| PathBuf::from("darwin-v6-fee-routing-controller.masp"))
}

fn main() {
    // The placeholder slot 11 felts in the MASM source above need to
    // be replaced with the real hashed values before assembly. This
    // binary is a stub so the v6 design is committed alongside v5;
    // running the assembly itself requires the SLOT_11_*_FELT consts
    // resolved.
    if SLOT_11_SUFFIX == 0 && SLOT_11_PREFIX == 0 {
        eprintln!(
            "v6 is a stub — populate SLOT_11_SUFFIX/PREFIX from compute_slot_ids \
             (run with slot name \"darwin::slot_11\") and substitute the constants \
             into the MASM source before assembling."
        );
        std::process::exit(1);
    }

    let out_path = parse_args();
    let source_manager: Arc<dyn miden_assembly::SourceManager> =
        Arc::new(DefaultSourceManager::default());

    let math_module = Module::parser(ModuleKind::Library)
        .parse_str(
            Path::new(MATH_NAMESPACE),
            darwin_protocol_account::MATH_MASM,
            source_manager.clone(),
        )
        .expect("darwin::math parses");

    let math_lib = miden_protocol::transaction::TransactionKernel::assembler()
        .assemble_library([math_module])
        .expect("darwin::math assembles");

    let source_resolved = V6_CONTROLLER_SOURCE
        .replace("SLOT_11_SUFFIX_FELT", &SLOT_11_SUFFIX.to_string())
        .replace("SLOT_11_PREFIX_FELT", &SLOT_11_PREFIX.to_string());

    let controller_module = Module::parser(ModuleKind::Library)
        .parse_str(
            Path::new(CONTROLLER_NAMESPACE),
            &source_resolved,
            source_manager,
        )
        .expect("v6 controller parses");

    let controller_lib = miden_protocol::transaction::TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())
        .expect("darwin::math attaches")
        .assemble_library([controller_module])
        .expect("v6 controller assembles");

    println!("v6 controller procedures (MAST roots):");
    for mi in controller_lib.module_infos() {
        for (_, pi) in mi.procedures() {
            let bytes: Vec<u8> = pi
                .digest
                .as_elements()
                .iter()
                .flat_map(|f| f.as_canonical_u64().to_le_bytes())
                .collect();
            let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
            println!("  {}::{:<28} call.0x{}", mi.path(), pi.name, hex);
        }
    }

    let metadata = AccountComponentMetadata::new(
        "darwin-basket-controller-v6-fee-routing",
        [AccountType::RegularAccountImmutableCode],
    )
    .with_description(
        "Darwin Protocol Account controller v6. Adds slot 11 \
         fee_recipient (set/get) and compound receive_and_credit \
         proc. Strict superset of v5 — receive_asset + all v3/v4/v5 \
         MAST roots preserved.",
    );
    let metadata_bytes = metadata.to_bytes();

    let mut package = Package::from_library(
        PackageId::from("darwin-basket-controller-v6"),
        Version::new(0, 6, 0),
        TargetType::AccountComponent,
        controller_lib,
        std::iter::empty(),
    );
    package.sections.push(Section::new(
        SectionId::ACCOUNT_COMPONENT_METADATA,
        metadata_bytes,
    ));
    package
        .write_to_file(&out_path)
        .unwrap_or_else(|e| panic!("write {}: {}", out_path.display(), e));
    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    println!();
    println!("Wrote {} ({size} bytes)", out_path.display());
}
