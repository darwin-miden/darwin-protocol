//! Darwin Protocol Account — one private `RegularAccountImmutableCode`
//! per basket.
//!
//! Implements the `DarwinBasketController` component declared in
//! `asm/controller.masm` and provides Rust helpers to deploy the account
//! against a Miden testnet.
//!
//! The MASM library is built at build time once the Miden toolchain
//! dependency is enabled in the workspace `Cargo.toml`. Until then, this
//! crate exposes the storage-slot layout, the controller's procedure
//! surface, and a builder API as plain Rust types.

pub mod component;
pub mod storage;

pub use component::DarwinBasketController;
pub use storage::StorageLayout;

/// Re-exports of the Miden objects types that the controller's public
/// surface uses. Keeping a single re-export point here makes future
/// migrations across miden-base / miden-objects breaking changes
/// easier to track.
///
/// v0.15: `AccountType` moved over to `miden_protocol::account` (it now
/// names the storage mode `{ Private, Public }` instead of the old kind
/// enum like `RegularAccountImmutableCode` / `FungibleFaucet`). We
/// re-export from miden_protocol so call sites that say
/// `miden::AccountType::Public` resolve correctly.
pub mod miden {
    pub use miden_objects::account::{
        Account, AccountBuilder, AccountId, SlotName, StorageMap, StorageSlot,
    };
    pub use miden_protocol::account::AccountType;
}

/// Resolve the Miden RPC endpoint from the `MIDEN_NETWORK` env var.
///
/// Defaults to **testnet** so existing scripts run unchanged. Set
/// `MIDEN_NETWORK=devnet` to point every deploy/flow binary at
/// `rpc.devnet.miden.io` (the v0.15 network that shipped 2026-06-19),
/// or `MIDEN_NETWORK=localhost` for a local node.
///
/// Confirmed endpoints (probed 2026-06-19):
///   - testnet: `https://rpc.testnet.miden.io`
///   - devnet:  `https://rpc.devnet.miden.io`
///       (faucet: `https://faucet.devnet.miden.io`,
///        explorer: `https://explorer.devnet.miden.io`)
pub fn miden_endpoint() -> miden_client::rpc::Endpoint {
    match std::env::var("MIDEN_NETWORK")
        .ok()
        .as_deref()
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("devnet") => miden_client::rpc::Endpoint::devnet(),
        Some("localhost") | Some("local") => miden_client::rpc::Endpoint::localhost(),
        _ => miden_client::rpc::Endpoint::testnet(),
    }
}

/// Re-exports of the basket manifest types this controller depends on.
pub use darwin_baskets::{BasketManifest, Constituent};

/// MASM source for the controller. Loaded at compile time so the build
/// stays hermetic with the Rust crate.
pub const CONTROLLER_MASM: &str = include_str!("../asm/controller.masm");

/// MASM source for `darwin::math`. Convenient for integration tests
/// that need to re-parse the module rather than load the assembled
/// library.
pub const MATH_MASM: &str = include_str!("../asm/lib/math.masm");

/// Pre-assembled MASM artefacts produced by `build.rs`.
///
/// `PRIMITIVES_MASL` bundles the four math libraries (`darwin::nav`,
/// `darwin::mint`, `darwin::fees`, `darwin::redeem`) into one MAST.
/// `FLOW_MASL` is the higher-level composition library (`darwin::flow`)
/// that depends on the primitives.
///
/// Consumers load these with `miden_assembly::Library::read_from_bytes`
/// and attach them to an `Assembler` via `with_static_library` before
/// assembling note scripts or account components that reference the
/// `darwin::*` procedures.
pub const PRIMITIVES_MASL: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/darwin-primitives.masl"));
pub const FLOW_MASL: &[u8] = include_bytes!(concat!(env!("OUT_DIR"), "/darwin-flow.masl"));

/// Loads the bundled `darwin::nav`/`mint`/`fees`/`redeem` library.
pub fn primitives_library() -> miden_assembly::Library {
    use miden_assembly::serde::Deserializable;
    miden_assembly::Library::read_from_bytes(PRIMITIVES_MASL)
        .expect("bundled darwin primitives library deserialises")
}

/// Loads the bundled `darwin::flow` library.
pub fn flow_library() -> miden_assembly::Library {
    use miden_assembly::serde::Deserializable;
    miden_assembly::Library::read_from_bytes(FLOW_MASL)
        .expect("bundled darwin flow library deserialises")
}

/// Assemble the permissionless drip note script, linking the vendored
/// miden-standards libraries it depends on (BasicWallet, note_tag, P2ID). Every
/// binary that needs the drip note's script root — `deploy_dispenser` (to
/// allowlist it), `build_drip_note` (the /faucet API), `debug_drip` (local
/// exec) — goes through here so the root never drifts between them.
///
/// `dusdc_prefix` / `dusdc_suffix` are the dUSDC faucet id felts and
/// `drip_amount` the fixed payout in base units; they template into the script.
/// (Assembly ignores the templated values, so placeholders are fine when only
/// the root is needed — but pass the real values on the paths that emit notes.)
pub fn drip_note_script(
    dusdc_prefix: u64,
    dusdc_suffix: u64,
    drip_amount: u64,
) -> Result<miden_client::note::NoteScript, Box<dyn std::error::Error>> {
    use miden_protocol::transaction::TransactionKernel;

    // Link the whole assembled miden-standards library. This resolves
    // `p2id::new`, `note_tag::create_account_target`, and
    // `wallet::move_asset_to_note` to the CANONICAL standard scripts — so the
    // payout the drip creates carries the exact P2ID script root the network and
    // MidenFi recognise (a hand-vendored p2id assembles to a different root,
    // which the requester's wallet would not treat as a standard payment).
    let std_lib = miden_standards::StandardsLib::default();

    let src = darwin_notes::DRIP_NOTE_MASM
        .replace("{{DRIP_AMOUNT}}", &drip_amount.to_string())
        .replace("{{DUSDC_FAUCET_PREFIX}}", &dusdc_prefix.to_string())
        .replace("{{DUSDC_FAUCET_SUFFIX}}", &dusdc_suffix.to_string());

    let program = TransactionKernel::assembler()
        .with_static_library(std_lib.as_ref())?
        .assemble_program(&src)?;

    Ok(miden_client::note::NoteScript::new(program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn controller_masm_is_non_empty() {
        assert!(!CONTROLLER_MASM.trim().is_empty());
    }

    #[test]
    fn storage_layout_matches_spec() {
        let layout = StorageLayout::default();
        assert_eq!(layout.version_slot, 0);
        assert_eq!(layout.basket_faucet_id_slot, 1);
        assert_eq!(layout.pool_positions_slot, 2);
        assert_eq!(layout.target_weights_slot, 3);
        assert_eq!(layout.last_nav_slot, 4);
        assert_eq!(layout.last_nav_timestamp_slot, 5);
        assert_eq!(layout.pending_ops_slot, 6);
        assert_eq!(layout.fee_accrual_slot, 7);
        assert_eq!(layout.oracle_adapter_id_slot, 8);
        assert_eq!(layout.manifest_version_slot, 9);
    }

    #[test]
    fn controller_can_be_built_from_core_crypto_manifest() {
        let manifest = darwin_baskets::core_crypto();
        let _controller = DarwinBasketController::from_manifest(&manifest);
    }

    #[test]
    fn miden_account_types_are_re_exported() {
        // Smoke test: the re-export module is wired up and the types
        // resolve. The actual deployment binary uses these directly.
        let _: Option<miden::AccountType> = None;
        let _: Option<miden::AccountType> = None;
    }

    #[test]
    fn miden_assembler_is_wireable() {
        // Sanity check that miden-assembly is a working dependency.
        let _assembler = miden_assembly::Assembler::default();
    }

    #[test]
    fn bundled_primitives_library_loads() {
        let lib = primitives_library();
        // The deserialised library exposes at least one MAST root.
        assert!(lib.module_infos().count() > 0);
    }

    #[test]
    fn bundled_flow_library_loads() {
        let lib = flow_library();
        assert!(lib.module_infos().count() > 0);
    }

    #[test]
    fn stub_account_component_compiles() {
        // v0.15: account kind is derived from the bundled components and
        // `supported_types` no longer returns the old kind enum
        // (`RegularAccountImmutableCode`, `FungibleFaucet`, …) — there's
        // nothing to assert about it. Compilation alone is the smoke.
        let manifest = darwin_baskets::core_crypto();
        let controller = DarwinBasketController::from_manifest(&manifest);
        let _component = controller.account_component_stub().expect("compiles");
    }
}
