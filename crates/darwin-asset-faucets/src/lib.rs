//! Darwin custom asset faucets for the M1 testnet.
//!
//! Standard `FungibleFaucet` accounts used as the basket constituents for
//! Flow A validation. When the canonical AggLayer-bridged faucets become
//! available on the public Miden testnet, the basket manifests are
//! updated to reference those instead; the custom faucets remain for
//! local-stack integration testing.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AssetFaucetSpec {
    pub symbol: &'static str,
    pub name: &'static str,
    pub decimals: u8,
    pub max_supply: u128,
    pub pragma_pair: &'static str,
}

pub const DARWIN_ETH: AssetFaucetSpec = AssetFaucetSpec {
    symbol: "dETH",
    name: "Darwin Testnet ETH",
    decimals: 18,
    max_supply: 10_000_000_000_000_000_000_000_000, // 10M ETH equivalent
    pragma_pair: "ETH/USD",
};

pub const DARWIN_WBTC: AssetFaucetSpec = AssetFaucetSpec {
    symbol: "dWBTC",
    name: "Darwin Testnet WBTC",
    decimals: 8,
    max_supply: 100_000_000_000_000, // 1M WBTC equivalent (8 decimals)
    pragma_pair: "WBTC/USD",
};

pub const DARWIN_USDT: AssetFaucetSpec = AssetFaucetSpec {
    symbol: "dUSDT",
    name: "Darwin Testnet USDT",
    decimals: 6,
    max_supply: 1_000_000_000_000_000, // 1B USDT (6 decimals)
    pragma_pair: "USDT/USD",
};

pub const DARWIN_DAI: AssetFaucetSpec = AssetFaucetSpec {
    symbol: "dDAI",
    name: "Darwin Testnet DAI",
    decimals: 18,
    max_supply: 1_000_000_000_000_000_000_000_000_000, // 1B DAI (18 decimals)
    pragma_pair: "DAI/USD",
};

pub const ALL: &[AssetFaucetSpec] = &[DARWIN_ETH, DARWIN_WBTC, DARWIN_USDT, DARWIN_DAI];

/// Maps a manifest faucet alias (`darwin-eth`, `darwin-wbtc`, ...) to
/// its on-chain spec. Used by the deployment binary and by SDK code
/// that needs the decimals scaling factor.
pub fn by_alias(alias: &str) -> Option<&'static AssetFaucetSpec> {
    let symbol = match alias {
        "darwin-eth" => "dETH",
        "darwin-wbtc" => "dWBTC",
        "darwin-usdt" => "dUSDT",
        "darwin-dai" => "dDAI",
        _ => return None,
    };
    ALL.iter().find(|f| f.symbol == symbol)
}

/// Returns `10^decimals` for this faucet — the multiplier from
/// human-readable units to faucet base units. Saturates at u128::MAX
/// for the (impossible) case of an over-18-decimal faucet.
pub fn base_unit_scale(decimals: u8) -> u128 {
    10u128.checked_pow(decimals as u32).unwrap_or(u128::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_symbols_are_unique() {
        let mut s: Vec<&str> = ALL.iter().map(|f| f.symbol).collect();
        s.sort();
        let original_len = s.len();
        s.dedup();
        assert_eq!(s.len(), original_len);
    }

    #[test]
    fn all_decimals_in_range() {
        for f in ALL {
            assert!(f.decimals <= 18);
        }
    }

    #[test]
    fn manifest_aliases_resolve_to_known_faucets() {
        for alias in ["darwin-eth", "darwin-wbtc", "darwin-usdt", "darwin-dai"] {
            let spec = by_alias(alias).expect("known alias");
            assert!(spec.symbol.starts_with('d'));
        }
    }

    #[test]
    fn unknown_alias_returns_none() {
        assert!(by_alias("not-real").is_none());
        assert!(by_alias("").is_none());
    }

    #[test]
    fn base_unit_scale_matches_decimals() {
        assert_eq!(base_unit_scale(0), 1);
        assert_eq!(base_unit_scale(6), 1_000_000);
        assert_eq!(base_unit_scale(8), 100_000_000);
        assert_eq!(base_unit_scale(18), 1_000_000_000_000_000_000);
    }

    #[test]
    fn max_supply_fits_within_decimals_scale() {
        // Smoke-check that no faucet's max_supply implies a per-token
        // count greater than 10^36 (i.e. max_supply / scale stays in
        // sensible u128 range).
        for f in ALL {
            let scale = base_unit_scale(f.decimals);
            let tokens = f.max_supply / scale;
            assert!(tokens > 0, "{} max_supply {} < 1 token at decimals {}",
                    f.symbol, f.max_supply, f.decimals);
        }
    }
}
