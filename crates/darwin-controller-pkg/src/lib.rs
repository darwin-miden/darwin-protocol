//! Darwin Basket Controller — Rust→Miden account component.
//!
//! Spec: darwin-docs/m1-architecture-spec.md §5.
//!
//! Compiled by `cargo miden build` into `darwin_controller_pkg.masp`
//! which `miden client new-account` deploys as a
//! `RegularAccountUpdatableCode` account.
//!
//! Today the procedure bodies do as much of the spec math as fits in
//! the Wasm→MASM lowering pipeline without producing F32Const
//! opcodes (which the cargo-miden pipeline does not yet handle).
//! Specifically:
//!
//!   - `apply_deposit` / `apply_redeem` update a per-asset position
//!     by adding / subtracting the amount.
//!   - `compute_nav`, `compute_mint_amount`, `compute_redeem_amount`,
//!     and `accrue_management_fee` express their formula as a single
//!     in-circuit multiplication. The off-chain caller pre-computes
//!     the divisor inverse (or a per-block multiplier) and passes it
//!     as an input — this keeps the lowering free of `u64` division.
//!
//! Real storage-backed bodies (StorageMap reads, cross-component
//! calls to the oracle adapter) land once the ecosystem-version skew
//! between `miden-objects` 0.12 and `miden-assembly` 0.23 is
//! resolved.

#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use miden::{component, Felt};

#[component]
struct DarwinBasketController;

#[component]
impl DarwinBasketController {
    /// Spec §5.3 — compute the basket NAV. Returns
    /// `pool_value * supply_inverse_hint` (the caller supplies both),
    /// which is the production formula factored to avoid in-circuit
    /// u64 division.
    pub fn compute_nav(&self, pool_value: Felt, supply_inverse_hint: Felt) -> Felt {
        pool_value * supply_inverse_hint
    }

    /// Spec §5.3 — record a deposit. Returns the new per-asset
    /// position (`prior_position + amount`).
    pub fn apply_deposit(&self, prior_position: Felt, amount: Felt) -> Felt {
        prior_position + amount
    }

    /// Spec §5.3 — record a redeem. Returns the new per-asset
    /// position (`prior_position - amount`). Caller is responsible
    /// for the underflow check.
    pub fn apply_redeem(&self, prior_position: Felt, amount: Felt) -> Felt {
        prior_position - amount
    }

    /// Spec §6.3 — pro-rata mint formula. Returns
    /// `deposit_value * net_bps_factor`, where
    /// `net_bps_factor = (10000 - fee_bps) * supply / (10000 * nav)`
    /// is pre-computed off-chain.
    pub fn compute_mint_amount(&self, deposit_value: Felt, net_bps_factor: Felt) -> Felt {
        deposit_value * net_bps_factor
    }

    /// Spec §6.5 — inverse of compute_mint_amount.
    /// Returns `burn_amount * gross_release_factor`.
    pub fn compute_redeem_amount(&self, burn_amount: Felt, gross_release_factor: Felt) -> Felt {
        burn_amount * gross_release_factor
    }

    /// Spec §6.4 — streamed management-fee accrual. Returns
    /// `elapsed_blocks * fee_per_block_x_value` (caller pre-computes
    /// the per-block multiplier).
    pub fn accrue_management_fee(&self, elapsed_blocks: Felt, fee_per_block_x_value: Felt) -> Felt {
        elapsed_blocks * fee_per_block_x_value
    }

    /// Spec §5.2 slot 3 — read the target weight for a faucet id.
    /// Stub today (returns the input); production reads slot 3 of
    /// the protocol-account StorageMap.
    pub fn read_target_weight(&self, faucet_id: Felt) -> Felt {
        faucet_id
    }

    /// Spec §5.3 — admin rotate the oracle adapter pointer. Stub:
    /// returns the new adapter id. Production writes slot 8 and
    /// emits an admin event.
    pub fn update_oracle_adapter(&self, new_adapter_id: Felt) -> Felt {
        new_adapter_id
    }
}
