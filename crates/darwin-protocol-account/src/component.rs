//! High-level Rust representation of the `DarwinBasketController` MASM
//! component.
//!
//! Used by the SDK and deployment scripts to build a Miden account from a
//! basket manifest. Once the workspace adds `miden-base` as a dependency,
//! this struct will own the construction of the actual on-chain
//! `AccountComponent` (with the merged MASM library + initial storage).

use darwin_baskets::BasketManifest;

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

    /// The set of MASM procedures the controller exposes to note scripts
    /// and to administrative transactions. Spec §5.3.
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
}
