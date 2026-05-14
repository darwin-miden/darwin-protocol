//! Note scripts consumed by the Darwin Protocol Account.
//!
//! - `DEPOSIT_NOTE_MASM`: mint flow (Flow A).
//! - `REDEEM_NOTE_MASM`:  burn flow (Miden-side of Flow C).
//!
//! The MASM sources are bundled at compile time; once the Miden toolchain
//! is enabled in the workspace, `serialise_to_masp()` will build the
//! `.masp` packages consumed by miden-client.

pub const DEPOSIT_NOTE_MASM: &str = include_str!("../asm/deposit_note.masm");
pub const REDEEM_NOTE_MASM: &str = include_str!("../asm/redeem_note.masm");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DarwinNote {
    Deposit,
    Redeem,
}

impl DarwinNote {
    pub fn masm_source(self) -> &'static str {
        match self {
            DarwinNote::Deposit => DEPOSIT_NOTE_MASM,
            DarwinNote::Redeem => REDEEM_NOTE_MASM,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn note_sources_are_non_empty() {
        assert!(!DEPOSIT_NOTE_MASM.trim().is_empty());
        assert!(!REDEEM_NOTE_MASM.trim().is_empty());
    }

    #[test]
    fn note_sources_differ() {
        assert_ne!(DEPOSIT_NOTE_MASM, REDEEM_NOTE_MASM);
    }
}
