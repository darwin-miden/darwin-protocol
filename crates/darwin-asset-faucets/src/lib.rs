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
}
