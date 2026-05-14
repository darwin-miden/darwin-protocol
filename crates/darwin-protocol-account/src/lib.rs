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
pub use storage::{StorageLayout, StorageSlot};

/// Re-exports of the basket manifest types this controller depends on.
pub use darwin_baskets::{BasketManifest, Constituent};

/// MASM source for the controller. Loaded at compile time so the build
/// stays hermetic with the Rust crate.
pub const CONTROLLER_MASM: &str = include_str!("../asm/controller.masm");

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
}
