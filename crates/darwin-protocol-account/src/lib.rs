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
pub mod miden {
    pub use miden_objects::account::{
        Account, AccountBuilder, AccountId, AccountStorageMode, AccountType, SlotName, StorageMap,
        StorageSlot,
    };
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
        let _: Option<miden::AccountStorageMode> = None;
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
}
