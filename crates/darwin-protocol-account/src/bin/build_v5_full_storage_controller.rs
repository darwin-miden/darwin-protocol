//! Build a `.masp` for the v5 Darwin Protocol Account controller.
//!
//! v5 = v4 + the two storage layers the proposal §M1.1/M1.2 call for
//! but that earlier controllers deferred to M4:
//!
//!   1.4a — token weights / fee config in StorageMap on Miden
//!          (slot 3: target_weights, slot 4: fees). No more
//!          Sepolia-side basket config dependency for the deposit
//!          / redeem math.
//!
//!   1.4b — per-user position StorageMap (slot 10) keyed by user_id
//!          word. credit_user / debit_user / get_user_position
//!          live in the controller; the atomic notes call into them
//!          on consume so the controller is the authoritative ledger
//!          for both Miden-native and ETH-via-relay users.
//!
//! Strict superset of v4 — every v2/v3/v4 MAST root is preserved so
//! existing notes work unchanged against this account code.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account \
//!         --bin build_v5_full_storage_controller -- \
//!         --out /tmp/darwin-v5-full-storage-controller.masp

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

const V5_CONTROLLER_SOURCE: &str = r#"
use darwin::math
use miden::protocol::native_account
use miden::protocol::active_account

# ---------------------------------------------------------------------
# Storage layout (mirror darwin-protocol-account/src/storage.rs).
#
#   0  VERSION
#   1  BASKET_FAUCET_ID
#   2  pool_positions       StorageMap[faucet_id_word → u64 amount]
#   3  target_weights       StorageMap[basket_id_word → 3 × u64 bps]    (1.4a)
#   4  fees                 StorageMap[basket_id_word → 3 × u64 bps]    (1.4a)
#  10  user_positions       StorageMap[(user_id ‖ basket_id) → u64 amt] (1.4b)
#
# Weights / fees are packed into a single word (4 felts) as
# [w0, w1, w2, padding] respectively [mint_bps, redeem_bps, mgmt_bps, _].
# ---------------------------------------------------------------------

# v2-compatible procs preserved.
pub proc compute_nav
    exec.math::felt_div
end
pub proc apply_deposit
    add
end
pub proc apply_redeem
    sub
end
pub proc compute_mint_amount
    exec.math::felt_div
end
pub proc compute_redeem_amount
    exec.math::felt_div
end
pub proc accrue_management_fee
    mul
end
pub proc receive_asset
    exec.native_account::add_asset
    dropw
end

# Storage slot IDs (computed off-chain via compute_slot_ids):
#   slot 2  pool_positions   suffix=811430137917007933        prefix=4481777022490664135
#   slot 3  target_weights   suffix=6486922254117069551       prefix=12444993101681295303
#   slot 4  fees             suffix=16076714866331093212      prefix=10941162321188629145
#   slot 10 user_positions   suffix=15366932551269667247      prefix=14059285908597291169
#
# Slot IDs in Miden are hash_string_to_word(name)[0..2], not the
# array index, so MASM bodies that touch a storage map must use the
# real hashed (suffix, prefix) felts at each call site.

# v3-compatible: read pool position from slot 2.
pub proc read_pool_position
    push.4481777022490664135 push.811430137917007933
    exec.active_account::get_map_item
end

# v4-compatible: rebalance trigger entry point (no-op compute).
pub proc execute_rebalance_step
    drop drop
end

# ---------------------------------------------------------------------
# v5 — 1.4a: basket config storage on Miden.
# ---------------------------------------------------------------------

#! Read the target-weight word for a basket.
pub proc get_target_weights
    push.12444993101681295303 push.6486922254117069551
    exec.active_account::get_map_item
end

#! Read the fee word for a basket.
pub proc get_fees
    push.10941162321188629145 push.16076714866331093212
    exec.active_account::get_map_item
end

#! Admin: write the target-weights word for a basket.
pub proc set_target_weights
    push.12444993101681295303 push.6486922254117069551
    exec.native_account::set_map_item
end

#! Admin: write the fees word for a basket.
pub proc set_fees
    push.10941162321188629145 push.16076714866331093212
    exec.native_account::set_map_item
end

# ---------------------------------------------------------------------
# v5 — 1.4b: per-user position storage on Miden.
# ---------------------------------------------------------------------

#! Read a user's basket-token position.
#! Stack on entry:   [user_basket_key_word(4), pad(12)]
#!   user_basket_key = (user_id_suffix, user_id_prefix, basket_id_suffix, basket_id_prefix)
#! Stack on exit:    [position_word(4)]
pub proc get_user_position
    push.14059285908597291169 push.15366932551269667247
    exec.active_account::get_map_item
end

#! Admin: write a user's basket position word (absolute set).
#! Stack on entry:   [user_basket_key_word(4), value_word(4), pad(8)]
#! Stack on exit:    [old_value_word(4), pad]
#!
#! The "credit" and "debit" semantics are layered above this in the
#! note script that calls into the controller: the note reads the
#! current position via `get_user_position`, computes the new value
#! off-stack, and calls `set_user_position` with the result. Keeping
#! the controller proc as a pure setter avoids the complex stack
#! juggling needed for in-MASM read-modify-write, and matches how
#! `set_target_weights` / `set_fees` operate.
pub proc set_user_position
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
    out.unwrap_or_else(|| PathBuf::from("darwin-v5-full-storage-controller.masp"))
}

fn main() {
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

    let math_lib =
        miden_protocol::transaction::TransactionKernel::assembler()
            .assemble_library([math_module])
            .expect("darwin::math assembles");

    let controller_module = Module::parser(ModuleKind::Library)
        .parse_str(
            Path::new(CONTROLLER_NAMESPACE),
            V5_CONTROLLER_SOURCE,
            source_manager,
        )
        .expect("v5 controller parses");

    let controller_lib =
        miden_protocol::transaction::TransactionKernel::assembler()
            .with_static_library(math_lib.as_ref())
            .expect("darwin::math attaches")
            .assemble_library([controller_module])
            .expect("v5 controller assembles");

    println!("v5 controller procedures (MAST roots):");
    for mi in controller_lib.module_infos() {
        for (_, pi) in mi.procedures() {
            let bytes: Vec<u8> = pi
                .digest
                .as_elements()
                .iter()
                .flat_map(|f| f.as_canonical_u64().to_le_bytes())
                .collect();
            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            println!("  {}::{:<30} call.0x{}", mi.path(), pi.name, hex);
        }
    }

    let metadata = AccountComponentMetadata::new(
        "darwin-basket-controller-v5-full-storage",
        [AccountType::RegularAccountImmutableCode],
    )
    .with_description(
        "Darwin Protocol Account controller v5 (full storage). \
         Adds target_weights (slot 3) + fees (slot 4) on-Miden basket \
         config, and user_positions (slot 10) StorageMap. Strict \
         superset of v4 — all prior MAST roots preserved.",
    );
    let metadata_bytes = metadata.to_bytes();

    let mut package = Package::from_library(
        PackageId::from("darwin-basket-controller-v5"),
        Version::new(0, 5, 0),
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
    println!();
    println!("Deploy with:");
    println!("  miden client new-account \\");
    println!("    --account-type regular-account-immutable-code \\");
    println!("    --packages {} \\", out_path.display());
    println!("    --storage-mode private \\");
    println!("    --deploy");
}
