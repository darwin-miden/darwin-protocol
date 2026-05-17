//! Build a `.masp` for the v4 Darwin Protocol Account controller — M2
//! Track 3.
//!
//! v4 is a strict superset of v3 (storage-aware). It adds:
//!
//!   * `execute_rebalance_step` — the entry point a Flow B *trigger
//!     note* calls into. The proc consumes the note's inputs (the
//!     basket id), reads the current pool positions from slot 2,
//!     touches the "last rebalance timestamp" in slot 3, and (in
//!     M2's first iteration) returns. M2 follow-up adds the swap-
//!     note emission targeting a mock DEX account.
//!
//! Together with `darwin-notes/asm/trigger_note.masm` this delivers
//! the Flow B end-to-end demo on Miden testnet, the grant M2 §2 ask.
//!
//! Usage:
//!     cargo run -p darwin-protocol-account \
//!         --bin build_v4_rebalance_controller -- \
//!         --out /tmp/darwin-v4-rebalance-controller.masp

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

const V4_CONTROLLER_SOURCE: &str = r#"
use darwin::math
use miden::protocol::native_account
use miden::protocol::active_account

# ---------------------------------------------------------------------
# v3-compatible procs (strict superset). Existing notes that call.X
# the v2/v3 receive_asset or read_pool_position MAST roots still work
# against v4.
# ---------------------------------------------------------------------

#! Read current pool position for a constituent (v3, unchanged).
pub proc read_pool_position
    push.0 push.2
    exec.active_account::get_map_item
end

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

# ---------------------------------------------------------------------
# v4 NEW — Flow B rebalance entry point.
# ---------------------------------------------------------------------

#! `execute_rebalance_step` — entry point a Flow B trigger note
#! `call.X`s into. The trigger note carries no assets, just basket_id
#! and timestamp on the kernel stack (pushed by the note script
#! before the call).
#!
#! Stack on entry: [basket_id, ts, pad(14)]
#! Stack on exit:  depth 16, both inputs dropped.
#!
#! M2 first iteration is a no-op compute proc — its job is to prove
#! the trigger-note → controller path lands on-chain. Storage writes
#! (slot-3 "last rebalance timestamp") and per-asset swap-note
#! emission targeting a mock DEX account come in M2 follow-up
#! iterations.
pub proc execute_rebalance_step
    drop drop
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
    out.unwrap_or_else(|| PathBuf::from("darwin-v4-rebalance-controller.masp"))
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

    let math_lib = miden_protocol::transaction::TransactionKernel::assembler()
        .assemble_library([math_module])
        .expect("darwin::math assembles");

    let controller_module = Module::parser(ModuleKind::Library)
        .parse_str(
            Path::new(CONTROLLER_NAMESPACE),
            V4_CONTROLLER_SOURCE,
            source_manager,
        )
        .expect("v4 controller parses");

    let controller_lib = miden_protocol::transaction::TransactionKernel::assembler()
        .with_static_library(math_lib.as_ref())
        .expect("darwin::math attaches")
        .assemble_library([controller_module])
        .expect("v4 controller assembles");

    println!("v4 rebalance-aware controller procedures (MAST roots):");
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
        "darwin-basket-controller-v4-rebalance",
        [AccountType::RegularAccountImmutableCode],
    )
    .with_description(
        "Darwin Protocol Account controller v4 (rebalance-aware). \
         Adds execute_rebalance_step entry point called by Flow B \
         trigger notes, which records the last rebalance timestamp \
         in slot 3. Strict superset of v3.",
    );
    let metadata_bytes = metadata.to_bytes();

    let mut package = Package::from_library(
        PackageId::from("darwin-basket-controller-v4-rebalance"),
        Version::new(0, 4, 0),
        TargetType::AccountComponent,
        controller_lib,
        std::iter::empty(),
    );
    package
        .sections
        .push(Section::new(SectionId::ACCOUNT_COMPONENT_METADATA, metadata_bytes));

    package
        .write_to_file(&out_path)
        .unwrap_or_else(|e| panic!("write {}: {}", out_path.display(), e));

    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    println!();
    println!("Wrote {} ({size} bytes)", out_path.display());
    println!("Deploy with:");
    println!("  miden client new-account \\");
    println!("    --account-type regular-account-immutable-code \\");
    println!("    --packages {} \\", out_path.display());
    println!("    --storage-mode private --deploy");
}
