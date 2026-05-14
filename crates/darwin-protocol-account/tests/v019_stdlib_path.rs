//! THE BREAKTHROUGH TEST.
//!
//! Hypothesis: miden-stdlib 0.19 already ships `std::math::u64::div`
//! with a real event handler (`U64_DIV_EVENT_NAME → handle_u64_div`
//! at miden-stdlib-0.19.1/src/lib.rs:79). So we don't need
//! miden-core-lib's u64 div at all — the same primitive lives in
//! the 0.19 stdlib and is fully compatible with miden-objects 0.12.
//!
//! If a program using `std::math::u64::div` compiles cleanly via the
//! v0.19 Assembler that miden-objects 0.12 bundles, the version skew
//! that blocks AccountComponent::compile is SOLVED. We just rewrite
//! darwin::math to import std::math::u64 and the entire on-chain
//! Flow A becomes unblocked.

use miden_stdlib::StdLibrary;

#[test]
fn miden_objects_assembler_compiles_program_using_stdlib_u64_div() {
    // miden-objects 0.12's bundled Assembler is miden-assembly 0.19.
    // Attach miden-stdlib 0.19 as a static library so symbols like
    // `std::math::u64::div` resolve at compile time.
    use miden_objects::assembly::Assembler;

    let stdlib = StdLibrary::default();
    let assembler = Assembler::default()
        .with_static_library(stdlib.as_ref())
        .expect("stdlib 0.19 attaches to the 0.19 assembler");

    // Exactly the body of darwin::math::felt_div, but importing the
    // stdlib u64 module instead of miden-core-lib's u64 module.
    let module_source = "
use.std::math::u64

const.TWO_POW_32=4294967296

#! Drop-in replacement for darwin::math::felt_div that uses the
#! miden-stdlib 0.19 u64 division.
export.felt_div
    u32split
    movup.2
    u32split
    exec.u64::div
    swap
    push.TWO_POW_32
    mul
    add
end
";

    // Assemble as a library (the unit we'll inject into Darwin's
    // AccountComponent).
    use miden_objects::assembly::LibraryPath;
    let path: LibraryPath = "darwin::math_v019".parse().unwrap();
    let source_manager = miden_objects::assembly::DefaultSourceManager::default();
    let library_source =
        miden_objects::assembly::Module::parser(miden_objects::assembly::ModuleKind::Library)
            .parse_str(path, module_source, &source_manager)
            .expect("library module parses on 0.19 syntax");

    let lib = assembler
        .assemble_library([library_source])
        .expect("library assembles with stdlib's u64 div");

    println!(
        "✓ darwin::math (stdlib u64 path) assembles cleanly on the v0.19 path"
    );
    println!("  Library has {} modules", lib.module_infos().count());
    let proc_names: Vec<String> = lib
        .module_infos()
        .flat_map(|m| {
            m.procedures()
                .map(|(_, p)| p.name.to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    println!("  Available procedures: {proc_names:?}");
}

#[test]
fn account_component_compiles_against_stdlib_u64_path() {
    // The killer test: an AccountComponent whose procedures call
    // std::math::u64::div via a Darwin library. This is exactly the
    // shape Flow A needs.
    use miden_objects::account::{AccountComponent, AccountType, StorageSlot};
    use miden_objects::assembly::{Assembler, LibraryPath, Module, ModuleKind};

    let stdlib = StdLibrary::default();

    // Build a darwin::math library that uses std::math::u64::div.
    let library_source = "
use.std::math::u64
const.TWO_POW_32=4294967296
export.felt_div
    u32split
    movup.2
    u32split
    exec.u64::div
    swap
    push.TWO_POW_32
    mul
    add
end
";

    let path: LibraryPath = "darwin::math_v019".parse().unwrap();
    let source_manager = miden_objects::assembly::DefaultSourceManager::default();
    let lib_module = Module::parser(ModuleKind::Library)
        .parse_str(path, library_source, &source_manager)
        .expect("darwin::math_v019 parses");

    let darwin_math = Assembler::default()
        .with_static_library(stdlib.as_ref())
        .unwrap()
        .assemble_library([lib_module])
        .expect("darwin::math_v019 assembles");

    // Now build a controller account component that calls our library.
    let controller_source = "
use.darwin::math_v019

#! compute_nav using real u64 division via the 0.19 path.
export.compute_nav_real
    exec.math_v019::felt_div
end

#! Identity passthroughs for the surface the SDK + note scripts
#! invoke; the real bodies live in the dedicated library.
export.apply_deposit
    add
end
export.apply_redeem
    sub
end
";

    let component_assembler = Assembler::default()
        .with_static_library(stdlib.as_ref())
        .unwrap()
        .with_static_library(&darwin_math)
        .unwrap();

    let storage_slots: Vec<StorageSlot> = (0..10).map(|_| StorageSlot::empty_value()).collect();
    let component =
        AccountComponent::compile(controller_source, component_assembler, storage_slots)
            .expect("AccountComponent compiles with stdlib u64 div + darwin::math_v019")
            .with_supported_type(AccountType::RegularAccountImmutableCode);

    println!(
        "✓ AccountComponent built on the 0.19 path with real u64 division. \
         supported_types={:?}",
        component.supported_types(),
    );
}
