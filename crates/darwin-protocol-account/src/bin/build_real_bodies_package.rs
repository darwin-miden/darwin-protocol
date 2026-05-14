//! Build a `.masp` Package containing a Darwin Protocol Account
//! controller with REAL u64-division bodies, ready to deploy via
//! `miden client new-account --packages ...`.
//!
//! Background:
//!
//! - The earlier deployments (recorded in
//!   `darwin-baskets/state/testnet.toml`) used a `.masp` produced by
//!   `cargo miden build` against `darwin-controller-pkg`. That path
//!   lowers Rust → Wasm → MASM and can't currently express u64 division
//!   (the lowering rejects intermediate f32 lowering / etc.).
//!
//! - This binary bypasses cargo-miden entirely. It hand-assembles
//!   `darwin::math` and a `darwin::controller` library using
//!   `miden-assembly 0.22` against `miden-core-lib 0.22` (the same
//!   versions `miden-client 0.14` consumes via `miden-protocol 0.14.5`),
//!   then wraps the result in a `Package` carrying the
//!   `AccountComponentMetadata` section the CLI expects.
//!
//! Usage:
//!
//!     cargo run -p darwin-protocol-account \
//!         --bin build_real_bodies_package -- \
//!         --out /tmp/darwin-real-bodies-controller.masp
//!
//! Then deploy with the standard CLI:
//!
//!     miden client new-account \
//!         --account-type regular-account-immutable-code \
//!         --packages /tmp/darwin-real-bodies-controller.masp \
//!         --storage-mode private \
//!         --deploy
//!
//! This produces a fresh on-chain Darwin Protocol Account whose
//! `compute_nav` / `compute_mint_amount` / `compute_redeem_amount`
//! procedures actually run u64 division — the controller bodies that
//! were previously stubbed because of the (resolved) version skew.

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{Assembler, DefaultSourceManager, Path};
use miden_mast_package::{Package, PackageId, Section, SectionId, TargetType, Version};
use miden_protocol::account::component::AccountComponentMetadata;
use miden_protocol::account::AccountType;
use miden_assembly::serde::Serializable;

const CONTROLLER_NAMESPACE: &str = "darwin::controller";
const MATH_NAMESPACE: &str = "darwin::math";

const CONTROLLER_SOURCE: &str = r#"
use darwin::math

# Spec §5.3 — compute the basket NAV.
# Stack on entry:   [pool_value_x1e8, supply]
# Stack on exit:    [nav_x1e8 = pool_value_x1e8 / supply]
pub proc compute_nav
    exec.math::felt_div
end

# Spec §5.3 — record a deposit.
# Stack on entry:   [prior_position, amount]
# Stack on exit:    [prior_position + amount]
pub proc apply_deposit
    add
end

# Spec §5.3 — record a redeem.
# Stack on entry:   [prior_position, amount]
# Stack on exit:    [prior_position - amount]
pub proc apply_redeem
    sub
end

# Spec §6.3 — pro-rata mint amount.
# Stack on entry:   [deposit_value_x1e8, nav_x1e8]
# Stack on exit:    [mint_amount]
pub proc compute_mint_amount
    exec.math::felt_div
end

# Spec §6.5 — inverse of compute_mint_amount.
# Stack on entry:   [burn_amount, scale]
# Stack on exit:    [release_amount]
pub proc compute_redeem_amount
    exec.math::felt_div
end

# Spec §6.4 — streamed management-fee accrual.
# Stack on entry:   [elapsed_blocks, fee_per_block_x_value]
# Stack on exit:    [accrued_fee = elapsed_blocks * fee_per_block_x_value]
pub proc accrue_management_fee
    mul
end
"#;

fn parse_args() -> PathBuf {
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" | "-o" => {
                out = Some(PathBuf::from(args.next().expect("--out needs a path")));
            }
            other => panic!("unknown flag {other}"),
        }
    }
    out.unwrap_or_else(|| PathBuf::from("darwin-real-bodies-controller.masp"))
}

fn main() {
    let out_path = parse_args();

    // 1. Build the source manager and assembler chain. We pull in
    //    miden-core-lib 0.22 so `darwin::math` can resolve
    //    `miden::core::math::u64::div`.
    let source_manager: Arc<dyn miden_assembly::SourceManager> =
        Arc::new(DefaultSourceManager::default());
    let core_lib = miden_core_lib::CoreLibrary::default();

    // 2. Assemble darwin::math.
    let math_module = Module::parser(ModuleKind::Library)
        .parse_str(
            Path::new(MATH_NAMESPACE),
            darwin_protocol_account::MATH_MASM,
            source_manager.clone(),
        )
        .expect("darwin::math parses");

    let math_lib = Assembler::default()
        .with_static_library(core_lib.as_ref())
        .expect("core lib attaches")
        .assemble_library([math_module])
        .expect("darwin::math assembles");

    // 3. Assemble darwin::controller against math.
    let controller_module = Module::parser(ModuleKind::Library)
        .parse_str(
            Path::new(CONTROLLER_NAMESPACE),
            CONTROLLER_SOURCE,
            source_manager.clone(),
        )
        .expect("darwin::controller parses");

    let controller_lib = Assembler::default()
        .with_static_library(core_lib.as_ref())
        .expect("core lib attaches")
        .with_static_library(math_lib.as_ref())
        .expect("darwin::math attaches")
        .assemble_library([controller_module])
        .expect("darwin::controller assembles");

    // 4. Build the AccountComponentMetadata that the CLI's
    //    `AccountComponent::from_package` will pull back out.
    let metadata = AccountComponentMetadata::new(
        "darwin-basket-controller",
        [AccountType::RegularAccountImmutableCode],
    )
    .with_description(
        "Darwin Protocol Account controller for DCC, DAG, DCO. \
         compute_nav, compute_mint_amount, compute_redeem_amount \
         all run real u64 division via miden::core::math::u64::div.",
    );
    let metadata_bytes = metadata.to_bytes();

    // 5. Wrap controller into a Package and stamp the metadata section.
    let mut package = Package::from_library(
        PackageId::from("darwin-basket-controller"),
        Version::new(0, 1, 0),
        TargetType::AccountComponent,
        controller_lib,
        std::iter::empty(),
    );
    package
        .sections
        .push(Section::new(SectionId::ACCOUNT_COMPONENT_METADATA, metadata_bytes));

    // 6. Write to disk.
    package
        .write_to_file(&out_path)
        .unwrap_or_else(|e| panic!("write {}: {}", out_path.display(), e));

    let size = std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0);
    println!("Wrote {} ({} bytes)", out_path.display(), size);
    println!("Package id: {}", package.name);
    println!(
        "Package version: {}.{}.{}",
        package.version.major, package.version.minor, package.version.patch
    );
    println!("MAST digest: {:?}", package.digest());
    println!();
    println!("To deploy on Miden testnet:");
    println!();
    println!("  miden client new-account \\");
    println!("    --account-type regular-account-immutable-code \\");
    println!("    --packages {} \\", out_path.display());
    println!("    --storage-mode private \\");
    println!("    --deploy");
    println!();
    println!("This produces a fresh Darwin Protocol Account whose compute_nav");
    println!("runs real u64 division — the controller body the v0.23 → v0.22");
    println!("workspace migration unblocked.");
}
