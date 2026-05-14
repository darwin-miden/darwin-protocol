//! Storage slot layout for the Darwin Protocol Account.
//!
//! Mirrors §5.2 of the M1 architecture specification. The slot positions
//! are part of the contract surface — they must NOT be changed without a
//! migration plan.
//!
//! The Rust struct `StorageLayout` documents the canonical slot indices.
//! `slot_for` returns a `miden_objects::SlotName` derived from the
//! protocol's MASM-side `const` names so cross-component callers and
//! tests can refer to slots symbolically.

use miden_objects::account::SlotName;

#[derive(Debug, Clone, Copy)]
pub struct StorageLayout {
    pub version_slot: u8,
    pub basket_faucet_id_slot: u8,
    pub pool_positions_slot: u8,
    pub target_weights_slot: u8,
    pub last_nav_slot: u8,
    pub last_nav_timestamp_slot: u8,
    pub pending_ops_slot: u8,
    pub fee_accrual_slot: u8,
    pub oracle_adapter_id_slot: u8,
    pub manifest_version_slot: u8,
}

impl Default for StorageLayout {
    fn default() -> Self {
        Self {
            version_slot: 0,
            basket_faucet_id_slot: 1,
            pool_positions_slot: 2,
            target_weights_slot: 3,
            last_nav_slot: 4,
            last_nav_timestamp_slot: 5,
            pending_ops_slot: 6,
            fee_accrual_slot: 7,
            oracle_adapter_id_slot: 8,
            manifest_version_slot: 9,
        }
    }
}

/// Symbolic names for each slot, matching the `const`s in
/// `asm/controller.masm`. Used by tests and indexers that prefer to
/// reference slots by name rather than by index.
pub mod names {
    pub const VERSION: &str = "darwin::controller::version";
    pub const BASKET_FAUCET_ID: &str = "darwin::controller::basket_faucet_id";
    pub const POOL_POSITIONS: &str = "darwin::controller::pool_positions";
    pub const TARGET_WEIGHTS: &str = "darwin::controller::target_weights";
    pub const LAST_NAV: &str = "darwin::controller::last_nav";
    pub const LAST_NAV_TIMESTAMP: &str = "darwin::controller::last_nav_timestamp";
    pub const PENDING_OPS: &str = "darwin::controller::pending_ops";
    pub const FEE_ACCRUAL: &str = "darwin::controller::fee_accrual";
    pub const ORACLE_ADAPTER_ID: &str = "darwin::controller::oracle_adapter_id";
    pub const MANIFEST_VERSION: &str = "darwin::controller::manifest_version";
}

/// Returns a `miden_objects::SlotName` for a given canonical slot name.
pub fn slot_for(name: &str) -> Result<SlotName, miden_objects::SlotNameError> {
    SlotName::new(name)
}
