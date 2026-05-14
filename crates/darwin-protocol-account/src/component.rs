//! Rust representation of the `DarwinBasketController` Miden component.
//!
//! This module is intentionally minimal today. The intent is to wrap
//! the bundled MASM library (`primitives_library()` + `flow_library()`)
//! into an [`miden_objects::account::AccountComponent`] suitable for
//! [`miden_objects::account::AccountBuilder`].
//!
//! # Ecosystem version mismatch (M1 blocker)
//!
//! The bundled library is assembled with `miden-assembly` 0.23 (matched
//! to `miden-vm` 0.23 and `miden-core-lib` 0.23, which provides the
//! u64 division event handler that `darwin::math::felt_div` depends
//! on). `miden-objects` 0.12 from crates.io is pinned to
//! `miden-assembly` 0.19; the `next` branch of `0xMiden/protocol`
//! (which renames the crate to `miden-protocol` 0.15) is on 0.22.
//! Neither aligns with 0.23.
//!
//! Passing a `miden-assembly` 0.23 `Library` to `AccountComponent::new`
//! fails to type-check because the `Library` types from different
//! versions of `miden-assembly-syntax` are distinct.
//!
//! Until the ecosystem realigns (most likely once `miden-protocol`
//! ships a release that bundles `miden-assembly` 0.23 alongside the
//! v0.14-alpha bridge work), `account_component()` is deferred to a
//! follow-up. The libraries themselves are fully assembled and
//! tested via `miden-vm` 0.23 (see `tests/*.rs`).

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

    /// The MASM procedures exposed by the controller (spec §5.3).
    /// These are the procedures that note scripts will invoke at
    /// runtime once the controller logic itself ships. They are
    /// implemented across the bundled `darwin::*` library modules.
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
