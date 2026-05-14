//! Storage slot layout for the Darwin Protocol Account.
//!
//! Mirrors §5.2 of the M1 architecture specification. The slot positions
//! are part of the contract surface — they must NOT be changed without a
//! migration plan.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StorageSlot(pub u8);

impl StorageSlot {
    pub fn index(self) -> u8 {
        self.0
    }
}

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
