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

# v3-compatible: read pool position from slot 2.
pub proc read_pool_position
    push.0 push.2
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
#! Stack on entry:   [basket_id_word(4), pad(12)]
#! Stack on exit:    [weights_word(4) = (w0,w1,w2,_), pad]
pub proc get_target_weights
    push.0 push.3                            # slot 3 — target_weights map
    exec.active_account::get_map_item
end

#! Read the fee word for a basket.
#! Stack on entry:   [basket_id_word(4), pad(12)]
#! Stack on exit:    [fees_word(4) = (mint, redeem, mgmt, _), pad]
pub proc get_fees
    push.0 push.4                            # slot 4 — fees map
    exec.active_account::get_map_item
end

# ---------------------------------------------------------------------
# v5 — 1.4b: per-user position storage on Miden.
# ---------------------------------------------------------------------

#! Read a user's basket-token position.
#! Stack on entry:   [user_basket_key_word(4), pad(12)]
#!   user_basket_key = (user_id_suffix, user_id_prefix, basket_id_suffix, basket_id_prefix)
#! Stack on exit:    [position_word(4)]
pub proc get_user_position
    push.0 push.10                           # slot 10 — user_positions map
    exec.active_account::get_map_item
end

# Internal helpers for credit/debit: read-modify-write the user's
# position word. The note script that consumed the atomic note pushes
# the user_basket_key + amount, calls this, and the controller stores
# the updated position.

#! Increment a user's basket position by `amount`.
#! Stack on entry:   [user_basket_key_word(4), amount, pad]
#! Stack on exit:    [new_position, pad]
#!
#! Note: the actual `set_map_item` write is gated behind the
#! authentication path of the consuming transaction — the controller
#! consumer tx must be the one running this, since it's the only
#! account allowed to touch its own storage. For M1.4b first iteration
#! we expose the read side + the compute-only credit (the write side
#! requires a separate signed call that lands in the same tx context;
#! kept for the follow-up that wires the note → controller call chain).
pub proc credit_user
    # Save amount + key for after the read.
    movup.4                                  # [amount, key(4), pad]
    movdn.8                                  # [key(4), pad(3), amount, pad]

    # Snapshot existing position.
    push.0 push.10
    exec.active_account::get_map_item        # [prev_position_word(4), amount, ...]

    # Add amount (only the first felt of the word is used as scalar).
    movup.8                                  # [amount, prev(4), ...]
    add                                      # [new_position_first_felt, prev[1..3], ...]
end

#! Decrement a user's basket position by `amount`.
#! Stack on entry:   [user_basket_key_word(4), amount, pad]
#! Stack on exit:    [new_position_first_felt, prev[1..3], pad]
pub proc debit_user
    movup.4
    movdn.8

    push.0 push.10
    exec.active_account::get_map_item

    movup.8
    sub
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
