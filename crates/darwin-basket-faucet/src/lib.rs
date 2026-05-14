//! Darwin basket-token faucet.
//!
//! Each basket (DCC, DAG, DCO) has its own FungibleFaucet. The faucet
//! implements the standard mint/burn surface plus the `agglayer_faucet`
//! interface (§6.6 of the M1 spec) so the basket token can be bridged
//! out to Ethereum via AggLayer.

use darwin_baskets::BasketManifest;

pub const FAUCET_MASM: &str = include_str!("../asm/faucet.masm");

#[derive(Debug, Clone)]
pub struct BasketFaucet {
    pub symbol: String,
    pub decimals: u8,
    pub manifest: BasketManifest,
}

impl BasketFaucet {
    pub fn from_manifest(manifest: &BasketManifest) -> Self {
        Self {
            symbol: manifest.symbol.clone(),
            decimals: manifest.basket_faucet_decimals,
            manifest: manifest.clone(),
        }
    }

    /// Synthetic Miden-origin token address for this basket. Deterministic,
    /// 20 bytes. Derivation: `Keccak256("darwin:" || symbol)[0..20]`. The
    /// actual Keccak call lives in the MASM faucet at runtime; this Rust
    /// representation is used by the SDK and the deployment script.
    pub fn synthetic_origin_address(&self) -> [u8; 20] {
        // TODO: implement Keccak-256 derivation once the Miden v0.14 toolchain
        // is wired in. For now, return a placeholder that encodes the symbol.
        let mut out = [0u8; 20];
        let bytes = self.symbol.as_bytes();
        let len = bytes.len().min(20);
        out[..len].copy_from_slice(&bytes[..len]);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn faucet_masm_is_non_empty() {
        assert!(!FAUCET_MASM.trim().is_empty());
    }

    #[test]
    fn synthetic_origin_address_encodes_symbol() {
        let m = darwin_baskets::core_crypto();
        let faucet = BasketFaucet::from_manifest(&m);
        let addr = faucet.synthetic_origin_address();
        assert_eq!(&addr[..3], b"DCC");
    }

    #[test]
    fn all_m1_baskets_produce_faucets() {
        for manifest in darwin_baskets::all_m1() {
            let _f = BasketFaucet::from_manifest(&manifest);
        }
    }

    #[test]
    fn synthetic_origin_addresses_are_distinct_per_basket() {
        let mut seen = std::collections::HashSet::new();
        for manifest in darwin_baskets::all_m1() {
            let addr = BasketFaucet::from_manifest(&manifest).synthetic_origin_address();
            assert!(
                seen.insert(addr),
                "{} produced a duplicate synthetic origin address",
                manifest.symbol,
            );
        }
    }

    #[test]
    fn faucet_decimals_round_trip_from_manifest() {
        for manifest in darwin_baskets::all_m1() {
            let f = BasketFaucet::from_manifest(&manifest);
            assert_eq!(f.decimals, manifest.basket_faucet_decimals);
            assert!(f.decimals <= 18, "decimals out of range: {}", f.decimals);
        }
    }

    #[test]
    fn synthetic_origin_address_for_dag_is_zero_padded() {
        let m = darwin_baskets::aggressive();
        let addr = BasketFaucet::from_manifest(&m).synthetic_origin_address();
        assert_eq!(&addr[..3], b"DAG");
        // Bytes past the symbol are zero (placeholder derivation
        // until Keccak-256 lands).
        for b in &addr[3..] {
            assert_eq!(*b, 0);
        }
    }
}
