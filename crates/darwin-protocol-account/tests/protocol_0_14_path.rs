//! THE FINAL PROOF.
//!
//! Build a real `miden_protocol::account::AccountComponent` from a
//! Darwin controller that calls `darwin::math::felt_div` (which uses
//! `miden::core::math::u64::div` via miden-core-lib 0.22). This is
//! the exact shape `miden-client 0.14` deploys to testnet. If it
//! compiles, M1 deliverable 5 (atomic Flow A) is one
//! `Client::add_account()` away.
//!
//! The "ecosystem version skew" story dies here: the math libraries
//! and the AccountComponent share a single MAST format (miden-assembly
//! 0.22) and the AccountComponent is the same type miden-client 0.14
//! consumes.

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{Assembler, DefaultSourceManager, Path};
use std::sync::Arc;

#[test]
fn miden_protocol_0_14_account_component_with_real_u64_division() {
    use miden_protocol::account::AccountComponent;
    use miden_protocol::account::component::AccountComponentMetadata;
    use miden_protocol::account::storage::{StorageSlot, StorageSlotName};
    use miden_protocol::account::AccountType;

    // 1. Assemble Darwin's primitives library using miden-assembly 0.22
    //    + miden-core-lib 0.22 — the same line miden-protocol 0.14 uses.
    let core_lib = miden_core_lib::CoreLibrary::default();
    let source_manager: Arc<dyn miden_assembly::SourceManager> =
        Arc::new(DefaultSourceManager::default());

    let math_source = darwin_protocol_account::MATH_MASM;
    let path = Path::new("darwin::math");
    let math_module = Module::parser(ModuleKind::Library)
        .parse_str(path, math_source, source_manager.clone())
        .expect("darwin::math parses on the 0.22 line");

    let primitives_lib = Assembler::default()
        .with_static_library(core_lib.as_ref())
        .expect("miden-core-lib 0.22 attaches")
        .assemble_library([math_module])
        .expect("darwin::math assembles on 0.22");

    // 2. Build a controller source that ACTUALLY uses darwin::math::felt_div
    //    inside its bodies — no more identity stubs, no more precomputed
    //    inverses. The real on-chain math.
    let controller_source = "
use darwin::math

pub proc compute_nav
    # Stack on entry:   [pool_value_x1e8, supply]
    # Stack on exit:    [nav_x1e8] = pool_value_x1e8 / supply
    exec.math::felt_div
end

pub proc compute_mint_amount
    # Stack on entry:   [deposit_value_x1e8, nav_x1e8]
    # Stack on exit:    [mint_amount]                     = deposit / nav
    exec.math::felt_div
end

pub proc compute_redeem_amount
    # Stack on entry:   [burn_amount, nav_x1e8_inv_scale]
    # Stack on exit:    [release_value]
    exec.math::felt_div
end

pub proc apply_deposit
    add
end

pub proc apply_redeem
    sub
end
";

    let controller_assembler = Assembler::default()
        .with_static_library(core_lib.as_ref())
        .expect("core lib attaches")
        .with_static_library(&primitives_lib)
        .expect("darwin primitives attaches");

    let controller_lib = controller_assembler
        .assemble_library({
            let path = Path::new("darwin::controller");
            let module = Module::parser(ModuleKind::Library)
                .parse_str(path, controller_source, source_manager.clone())
                .expect("controller parses");
            [module]
        })
        .expect("controller library assembles on 0.22 with real u64 division");

    // 3. Wrap as a miden-protocol 0.14 AccountComponent. This is the
    //    type miden-client::Client::add_account takes via AccountBuilder.
    let metadata = AccountComponentMetadata::new(
        "darwin-basket-controller",
        [AccountType::RegularAccountImmutableCode],
    );

    let storage_slots: Vec<StorageSlot> = (0..10)
        .map(|i| {
            let name = StorageSlotName::new(format!("darwin::slot_{i}")).unwrap();
            StorageSlot::with_empty_value(name)
        })
        .collect();

    // assemble_library returns Arc<Library>; AccountComponentCode wants Library.
    let controller_lib_owned: miden_assembly::Library = (*controller_lib).clone();
    let component =
        AccountComponent::new(controller_lib_owned, storage_slots, metadata)
            .expect("AccountComponent builds on miden-protocol 0.14");

    println!(
        "✓ miden-protocol 0.14 AccountComponent built with real u64 division. \
         storage_size={} procedures={}",
        component.storage_size(),
        component.procedures().count(),
    );

    // 4. Confirm the procedures we declared actually surface.
    let proc_count = component.procedures().count();
    assert!(proc_count >= 5, "expected 5+ procedures, got {proc_count}");
}
