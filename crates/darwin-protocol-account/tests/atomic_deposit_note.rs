//! Atomic DepositNote — proof that a NoteScript can call into
//! `darwin::math::felt_div` and produce a real Miden NoteScript.
//!
//! Combined with the deployed real-bodies controller (account
//! `0x171f46fecf1bca8005ae068a8dfe77`), this gives the two halves of
//! Flow A atomic: a note that the user submits, computing the mint
//! amount on-chain using real u64 division, and a controller that
//! consumes it.
//!
//! The real Flow A note would additionally:
//!   1. Move deposited assets from the note's vault into the account.
//!   2. Cross-component call the basket faucet to mint basket tokens.
//!   3. Write the new pool position into the controller's storage map.
//!
//! Those steps require kernel-aware MASM (`miden::note::*` /
//! `miden::account::*`) and the basket-faucet's `agglayer_faucet`
//! interface. They're the next implementation step. This test proves
//! the math + NoteScript wrapping path that everything else builds on.

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::{Assembler, DefaultSourceManager, Path};
use std::sync::Arc;

#[test]
fn atomic_deposit_note_assembles_with_darwin_math() {
    use miden_protocol::note::NoteScript;

    let core_lib = miden_core_lib::CoreLibrary::default();
    let source_manager: Arc<dyn miden_assembly::SourceManager> =
        Arc::new(DefaultSourceManager::default());

    // 1. Assemble darwin::math (the felt_div library that depends on
    //    miden-core-lib's u64::div event handler).
    let math_path = Path::new("darwin::math");
    let math_module = Module::parser(ModuleKind::Library)
        .parse_str(
            math_path,
            darwin_protocol_account::MATH_MASM,
            source_manager.clone(),
        )
        .expect("darwin::math parses");

    let math_lib = Assembler::default()
        .with_static_library(core_lib.as_ref())
        .expect("core lib attaches")
        .assemble_library([math_module])
        .expect("darwin::math assembles");

    // 2. The atomic deposit note's body. Today this is the math-only
    //    skeleton — the kernel-aware version comes later. The inputs
    //    correspond to spec §7.1: (deposit_value_x1e8, current_nav_x1e8,
    //    fee_factor_x1e8). The script computes
    //    mint_amount = (deposit_value * (10000 - fee_bps)) / (10000 * nav).
    let note_source = "
use darwin::math

begin
    # Stack on entry (in order popped from inputs):
    #   [deposit_value_x1e8, nav_x1e8, fee_factor]
    # Stack on exit:
    #   [mint_amount]

    # First: deposit_value * fee_factor (fee_factor already in 1e8 scale)
    mul                          # [deposit_value * fee_factor, nav_x1e8]
    exec.math::felt_div          # [mint_amount]
end
";

    let program = Assembler::default()
        .with_static_library(core_lib.as_ref())
        .expect("core lib attaches")
        .with_static_library(math_lib.as_ref())
        .expect("darwin::math attaches")
        .assemble_program(note_source)
        .expect("atomic deposit note assembles with darwin::math");

    // 3. Wrap as a miden-protocol 0.14 NoteScript — the exact type
    //    `miden-client::Client::new_transaction` accepts as a script.
    let note_script = NoteScript::new(program);

    println!("✓ Atomic deposit NoteScript assembled");
    println!("  root: {:?}", note_script.root());
    println!("  mast nodes: {}", note_script.mast().num_nodes());
    println!("  entrypoint: {:?}", note_script.entrypoint());

    // Serialize round-trip confirms the wire format is sound.
    use miden_assembly::serde::{Deserializable, Serializable};
    let bytes = note_script.to_bytes();
    let round_tripped = NoteScript::read_from_bytes(&bytes)
        .expect("NoteScript serialization round-trips");
    assert_eq!(round_tripped.root(), note_script.root());
    println!("  serialized: {} bytes", bytes.len());
}

#[test]
fn nav_math_runs_correctly_under_miden_vm() {
    // Sanity baseline: the math the atomic deposit note executes is
    // identical to the darwin::math::felt_div the existing test suite
    // exercises. This is the link between "the note script
    // assembles" and "the math actually produces the right answer".

    // (deposit_value=100, fee_factor=9970 [99.7%], nav=10000)
    // mint_amount = 100 * 9970 / 10000 = 99.7 ≈ 99 (integer division).

    use miden_vm::{
        advice::AdviceInputs, execute_sync, Assembler, DefaultHost, ExecutionOptions, StackInputs,
    };

    let core_lib = miden_core_lib::CoreLibrary::default();
    let primitives = darwin_protocol_account::primitives_library();

    let source = "
use darwin::math

begin
    push.10000                    # nav
    push.9970                     # fee_factor
    push.100                      # deposit_value
    mul
    exec.math::felt_div
    movup.15 drop
end
";

    let program = Assembler::default()
        .with_static_library(core_lib.as_ref())
        .unwrap()
        .with_static_library(&primitives)
        .unwrap()
        .assemble_program(source)
        .expect("program assembles");

    let mut host = DefaultHost::default()
        .with_library(&core_lib)
        .expect("core library handlers register");

    let outputs = execute_sync(
        &program,
        StackInputs::default(),
        AdviceInputs::default(),
        &mut host,
        ExecutionOptions::default(),
    )
    .expect("program executes");

    let result = outputs.stack_outputs()[0].as_canonical_u64();
    assert_eq!(result, 99, "100 * 9970 / 10000 should be 99, got {result}");
    println!("✓ Atomic deposit math: 100 * 9970 / 10000 = {result} (expected 99)");
}
