//! Rust representation of the `DarwinBasketController` Miden component.
//!
//! Two parallel paths exist:
//!
//! 1. **`account_component_stub`** — uses `miden-objects` 0.12's own
//!    `Assembler` (transitively `miden-assembly` 0.19) to compile the
//!    v0.14-line controller source in `asm/controller_v0_19.masm`.
//!    Procedure bodies are stubs but the resulting `AccountComponent`
//!    is real and usable by `miden-client`'s `AccountBuilder` —
//!    enough to actually deploy a Darwin Protocol Account placeholder
//!    on testnet today.
//!
//! 2. **The bundled `darwin::*` library** built by `build.rs` —
//!    assembled with `miden-assembly` 0.23 against `miden-core-lib`
//!    0.23 (which provides the u64-division event handler that
//!    `darwin::math::felt_div` depends on). These are the real math
//!    libraries used by `miden-vm` integration tests.
//!
//! The version skew between paths 1 and 2 (assembly 0.19 vs 0.23) is
//! the M1 blocker on wiring the full controller bodies into a real
//! account. Once `miden-protocol` ships a release that bundles
//! `miden-assembly` 0.23 alongside the v0.14-alpha bridge work, the
//! two paths converge: `account_component_stub` is replaced by a
//! version that takes the bundled library directly via
//! `AccountComponent::new`.

use darwin_baskets::BasketManifest;
use miden_objects::account::{AccountComponent, AccountType, StorageSlot};
use miden_objects::assembly::Assembler;
use miden_objects::AccountError;

/// MASM source for the v0.14-line ("ecosystem-current") controller.
/// Compiles cleanly against `miden_objects::assembly::Assembler`,
/// which uses `miden-assembly` 0.19 under the hood.
pub const CONTROLLER_V0_19_MASM: &str = include_str!("../asm/controller_v0_19.masm");

#[derive(Debug, Clone)]
pub struct DarwinBasketController {
    pub manifest: BasketManifest,
}

impl DarwinBasketController {
    pub fn from_manifest(manifest: &BasketManifest) -> Self {
        Self {
            manifest: manifest.clone(),
        }
    }

    /// The MASM procedures exposed by the controller (spec §5.3).
    /// These are the procedures that note scripts will invoke at
    /// runtime once the controller logic itself ships.
    pub fn procedure_surface() -> &'static [&'static str] {
        &[
            "compute_nav",
            "apply_deposit",
            "apply_redeem",
            "compute_mint_amount",
            "compute_redeem_amount",
            "accrue_management_fee",
            "read_target_weight",
            "update_oracle_adapter",
        ]
    }

    /// Builds a stub [`AccountComponent`] from the v0.14-line
    /// controller MASM. Procedure bodies are placeholders today, but
    /// the resulting component is wire-compatible with
    /// `miden-client`'s account deployment and demonstrates the
    /// procedure surface end-to-end on testnet.
    pub fn account_component_stub(&self) -> Result<AccountComponent, AccountError> {
        let storage_slots: Vec<StorageSlot> = (0..10).map(|_| StorageSlot::empty_value()).collect();
        AccountComponent::compile(CONTROLLER_V0_19_MASM, Assembler::default(), storage_slots)
            .map(|c| c.with_supported_type(AccountType::RegularAccountImmutableCode))
    }
}
