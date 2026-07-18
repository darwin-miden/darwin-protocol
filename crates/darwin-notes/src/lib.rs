//! Note scripts consumed by the Darwin Protocol Account.
//!
//! - `DEPOSIT_NOTE_MASM`: mint flow (Flow A).
//! - `REDEEM_NOTE_MASM`:  burn flow (Miden-side of Flow C).
//!
//! The MASM sources are bundled at compile time. Production
//! consumption goes through `NoteScript::fromPackage(.masp)` once the
//! `miden-objects` and `miden-tx` ecosystem stabilises on the
//! `miden-assembly` 0.23 line that the Darwin libraries target — until
//! then, `serialise_to_masp` is documented but unimplemented (see the
//! progress log in darwin-docs).

pub const DEPOSIT_NOTE_MASM: &str = include_str!("../asm/deposit_note.masm");
pub const REDEEM_NOTE_MASM: &str = include_str!("../asm/redeem_note.masm");
/// Permissionless dUSDC-dispenser drip request. Templated (faucet felts +
/// amount) at deploy time, then assembled to get its NoteScriptRoot.
pub const DRIP_NOTE_MASM: &str = include_str!("../asm/drip_note.masm");
/// Vendored copy of the miden-standards BasicWallet library, linked into the
/// assembler so the drip note can resolve `wallet::move_asset_to_note`. Same
/// source the dispenser's BasicWallet component uses → same proc root at runtime.
pub const STD_BASIC_WALLET_MASM: &str = include_str!("../asm/std_basic_wallet.masm");

/// Vendored miden-standards `miden::standards::notes::p2id` — the drip note
/// links it so `p2id::new` (create a PUBLIC P2ID payout, recipient details
/// recorded on-chain) resolves.
pub const STD_P2ID_MASM: &str = include_str!("../asm/std_p2id.masm");

/// Vendored miden-standards `miden::standards::note_tag` — the drip note links
/// it so `note_tag::create_account_target` (derive the requester's discovery
/// tag) resolves.
pub const STD_NOTE_TAG_MASM: &str = include_str!("../asm/std_note_tag.masm");

/// Self-contained atomic deposit note that runs real u64 division on
/// the deposit value. Wraps `darwin::math::felt_div`. Validated by
/// `darwin-protocol-account`'s `atomic_deposit_note.rs` integration
/// tests — the program assembles via miden-protocol 0.14's NoteScript
/// and the math executes correctly under miden-vm 0.22.
///
/// Compared to `DEPOSIT_NOTE_MASM` (the spec-level skeleton with
/// kernel calls), this is the minimal compute-only body that ships
/// today. The kernel-aware version (with note::move_assets +
/// basket_faucet::mint cross-account call) lands next.
pub const ATOMIC_DEPOSIT_NOTE_MASM: &str = include_str!("../asm/atomic_deposit_note.masm");

/// Atomic deposit note v2 — same as `ATOMIC_DEPOSIT_NOTE_MASM` plus
/// a call into the v5 controller's `set_user_position` proc to write
/// the credited basket-token amount into slot 10 (per-user positions
/// StorageMap). Requires the v5 controller to be deployed; doesn't
/// affect v2/v3/v4 consumers since the `receive_asset` MAST root is
/// unchanged.
///
/// Storage felts layout (5 felts):
///     [deposit_value, fee_factor, nav_scale, user_id_suffix, user_id_prefix]
pub const ATOMIC_DEPOSIT_NOTE_V2_MASM: &str =
    include_str!("../asm/atomic_deposit_note_v2.masm");

/// Atomic deposit note v3 — single `call.X` into v6's compound
/// `receive_and_credit` proc instead of v2's two-call shape
/// (receive_asset + set_user_position). Halves kernel transitions
/// per asset consumed; relies on v6 being the consuming controller.
pub const ATOMIC_DEPOSIT_NOTE_V3_MASM: &str =
    include_str!("../asm/atomic_deposit_note_v3.masm");

/// Self-contained atomic redeem note. Symmetric to
/// `ATOMIC_DEPOSIT_NOTE_MASM`: the user attaches basket-token assets
/// to the note; the script runs the redeem-value math via
/// `darwin::math::felt_div` then hands the basket tokens to the
/// controller via the controller's `receive_asset` proc. The on-chain
/// effect is "user surrenders basket tokens, the controller absorbs
/// them" — the M2 chains in an explicit basket-faucet `burn` call so
/// the supply ticks down too.
pub const ATOMIC_REDEEM_NOTE_MASM: &str = include_str!("../asm/atomic_redeem_note.masm");

/// Flow B trigger note — calls into the v4 controller's
/// `execute_rebalance_step` proc. Carries no assets, only metadata
/// (basket id + timestamp) encoded in the script constants.
///
/// Flow B trigger note used by the rebalance bot.
pub const REBALANCE_TRIGGER_NOTE_MASM: &str =
    include_str!("../asm/rebalance_trigger_note.masm");

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DarwinNote {
    Deposit,
    Redeem,
    /// Atomic deposit note — kernel-aware body that runs
    /// `darwin::math::felt_div` and drains the note's vault into the
    /// consuming controller.
    AtomicDeposit,
    /// Atomic redeem note — symmetric pair of AtomicDeposit.
    AtomicRedeem,
    /// Flow B trigger note — assetless, calls
    /// `controller::execute_rebalance_step`.
    RebalanceTrigger,
}

impl DarwinNote {
    pub fn masm_source(self) -> &'static str {
        match self {
            DarwinNote::Deposit => DEPOSIT_NOTE_MASM,
            DarwinNote::Redeem => REDEEM_NOTE_MASM,
            DarwinNote::AtomicDeposit => ATOMIC_DEPOSIT_NOTE_MASM,
            DarwinNote::AtomicRedeem => ATOMIC_REDEEM_NOTE_MASM,
            DarwinNote::RebalanceTrigger => REBALANCE_TRIGGER_NOTE_MASM,
        }
    }

    /// Canonical kebab-case identifier used in tooling logs and config
    /// files. Stable across DarwinNote renames in Rust.
    pub fn id(self) -> &'static str {
        match self {
            DarwinNote::Deposit => "deposit-note",
            DarwinNote::Redeem => "redeem-note",
            DarwinNote::AtomicDeposit => "atomic-deposit-note",
            DarwinNote::AtomicRedeem => "atomic-redeem-note",
            DarwinNote::RebalanceTrigger => "rebalance-trigger-note",
        }
    }

    /// Returns the imports the note script references. Useful for
    /// tooling that pre-resolves library dependencies before invoking
    /// the assembler.
    pub fn imported_libraries(self) -> &'static [&'static str] {
        match self {
            DarwinNote::Deposit => &[
                "darwin::basket_controller",
                "darwin::oracle_adapter",
                "darwin::basket_faucet",
                "miden::note",
                "miden::account",
            ],
            DarwinNote::Redeem => &[
                "darwin::basket_controller",
                "darwin::oracle_adapter",
                "darwin::basket_faucet",
                "miden::note",
                "miden::account",
            ],
            DarwinNote::AtomicDeposit | DarwinNote::AtomicRedeem => &[
                "darwin::math",
                "miden::protocol::active_note",
                "miden::protocol::asset",
            ],
            DarwinNote::RebalanceTrigger => &["miden::core::sys"],
        }
    }
}

/// Inputs the off-chain SDK serialises into a `DepositNote` before
/// submission. Mirrors §7.1 of the M1 spec.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct DepositNoteInputs {
    /// One (faucet_id, amount) pair per asset the user is depositing.
    pub assets: Vec<(u64, u64)>,
    /// The user's wallet that will receive the basket-token note.
    pub recipient_account_id: u64,
    /// Latest block at which this note may be consumed.
    pub expiry_block: u64,
}

/// Inputs the off-chain SDK serialises into a `RedeemNote`.
/// Mirrors §6.5 / §7.2 of the M1 spec.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RedeemNoteInputs {
    /// Amount of basket token to burn (basket-faucet base units).
    pub burn_amount: u64,
    /// Wallet that will receive the redeemed underlyings.
    pub recipient_account_id: u64,
    /// Latest block at which this note may be consumed.
    pub expiry_block: u64,
    /// Identifier of the basket being redeemed (the basket faucet id).
    pub basket_id: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_sources_are_non_empty() {
        assert!(!DEPOSIT_NOTE_MASM.trim().is_empty());
        assert!(!REDEEM_NOTE_MASM.trim().is_empty());
        assert!(!ATOMIC_DEPOSIT_NOTE_MASM.trim().is_empty());
        assert!(!ATOMIC_REDEEM_NOTE_MASM.trim().is_empty());
        assert!(!REBALANCE_TRIGGER_NOTE_MASM.trim().is_empty());
    }

    #[test]
    fn note_sources_differ() {
        assert_ne!(DEPOSIT_NOTE_MASM, REDEEM_NOTE_MASM);
        assert_ne!(DEPOSIT_NOTE_MASM, ATOMIC_DEPOSIT_NOTE_MASM);
        assert_ne!(ATOMIC_DEPOSIT_NOTE_MASM, ATOMIC_REDEEM_NOTE_MASM);
        assert_ne!(REBALANCE_TRIGGER_NOTE_MASM, ATOMIC_DEPOSIT_NOTE_MASM);
    }

    #[test]
    fn note_ids_are_stable_kebab_case() {
        assert_eq!(DarwinNote::Deposit.id(), "deposit-note");
        assert_eq!(DarwinNote::Redeem.id(), "redeem-note");
        assert_eq!(DarwinNote::AtomicDeposit.id(), "atomic-deposit-note");
        assert_eq!(DarwinNote::AtomicRedeem.id(), "atomic-redeem-note");
        assert_eq!(DarwinNote::RebalanceTrigger.id(), "rebalance-trigger-note");
    }

    #[test]
    fn rebalance_trigger_note_calls_v4_execute_rebalance_step() {
        // MAST root from build_v4_rebalance_controller's output. If
        // the v4 controller source changes, this assertion fails and
        // the note must be rebuilt with the new root.
        let expected_root =
            "0xddff122fa9aff9c1e5b5c253b509d24a795a9ad709f32d54e91eb53a77b84c53";
        assert!(
            REBALANCE_TRIGGER_NOTE_MASM.contains(&format!("call.{expected_root}")),
            "rebalance trigger note must call execute_rebalance_step"
        );
    }

    #[test]
    fn atomic_deposit_note_imports_darwin_math() {
        let source = DarwinNote::AtomicDeposit.masm_source();
        assert!(source.contains("use darwin::math"));
        assert!(source.contains("exec.math::felt_div"));
    }

    #[test]
    fn atomic_redeem_note_imports_darwin_math_and_drains_assets() {
        let source = DarwinNote::AtomicRedeem.masm_source();
        assert!(source.contains("use darwin::math"));
        assert!(source.contains("use miden::protocol::active_note"));
        assert!(source.contains("exec.math::felt_div"));
        // It must call into the controller's receive_asset MAST root
        // exactly once to drain the basket-token vault.
        assert!(
            source.contains("call.0x75f638c65584d058542bcf4674b066ae394183021bc9b44dc2fdd97d52f9bcfb"),
            "atomic redeem note must call the v2 controller's receive_asset"
        );
    }

    #[test]
    fn deposit_note_imports_match_source_use_statements() {
        for path in DarwinNote::Deposit.imported_libraries() {
            assert!(
                DEPOSIT_NOTE_MASM.contains(&format!("use.{path}"))
                    || DEPOSIT_NOTE_MASM.contains(&format!("use {path}")),
                "DepositNote source does not import {path}"
            );
        }
    }

    #[test]
    fn redeem_note_imports_match_source_use_statements() {
        for path in DarwinNote::Redeem.imported_libraries() {
            assert!(
                REDEEM_NOTE_MASM.contains(&format!("use.{path}"))
                    || REDEEM_NOTE_MASM.contains(&format!("use {path}")),
                "RedeemNote source does not import {path}"
            );
        }
    }

    #[test]
    fn deposit_inputs_round_trip_via_serde() {
        let inputs = DepositNoteInputs {
            assets: vec![(0xDEAD, 1_000), (0xBEEF, 500)],
            recipient_account_id: 0x1234,
            expiry_block: 695_500,
        };
        let json = serde_json::to_string(&inputs).expect("serialise");
        let decoded: DepositNoteInputs = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(decoded.assets, inputs.assets);
        assert_eq!(decoded.recipient_account_id, inputs.recipient_account_id);
        assert_eq!(decoded.expiry_block, inputs.expiry_block);
    }
}
