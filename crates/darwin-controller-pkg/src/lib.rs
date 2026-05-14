//! Darwin Basket Controller — Rust→Miden account component.
//!
//! Spec: darwin-docs/m1-architecture-spec.md §5.
//!
//! Compiled by `cargo miden build` into the
//! `darwin-protocol-account.masp` package that the deployment binary
//! ships with `miden client new-account`.
//!
//! All procedure bodies are placeholders (`a + b - b`) today — see the
//! M1 progress log for the dependency on miden-protocol shipping a
//! release built against miden-assembly 0.23 (at which point the
//! bodies are replaced by calls into the bundled `darwin::*` math
//! libraries).

#![no_std]
#![feature(alloc_error_handler)]

extern crate alloc;

use miden::{component, Felt};

#[component]
struct DarwinBasketController;

#[component]
impl DarwinBasketController {
    /// Spec §5.3 — compute the basket NAV. Stub: identity on input.
    pub fn compute_nav(&self, x: Felt) -> Felt {
        x
    }

    /// Spec §5.3 — record a deposit. Stub: returns sum.
    pub fn apply_deposit(&self, asset_key: Felt, asset_value: Felt) -> Felt {
        asset_key + asset_value
    }

    /// Spec §5.3 — record a redeem. Stub: returns sum.
    pub fn apply_redeem(&self, asset_key: Felt, asset_value: Felt) -> Felt {
        asset_key + asset_value
    }

    /// Spec §6.3 — pro-rata mint formula. Stub: returns deposit value.
    pub fn compute_mint_amount(&self, deposit_value: Felt, _pre_nav: Felt, _pre_supply: Felt) -> Felt {
        deposit_value
    }

    /// Spec §6.5 — inverse of compute_mint_amount. Stub: returns burn amount.
    pub fn compute_redeem_amount(&self, burn_amount: Felt, _nav: Felt, _supply: Felt) -> Felt {
        burn_amount
    }

    /// Spec §6.4 — streamed management-fee accrual. Stub.
    pub fn accrue_management_fee(&self, current_block: Felt) -> Felt {
        current_block
    }

    /// Spec §5.2 slot 3 — read the target weight for a faucet id. Stub.
    pub fn read_target_weight(&self, faucet_id: Felt) -> Felt {
        faucet_id
    }

    /// Spec §5.3 — admin rotate the oracle adapter pointer. Stub.
    pub fn update_oracle_adapter(&self, new_adapter_id: Felt) -> Felt {
        new_adapter_id
    }
}
