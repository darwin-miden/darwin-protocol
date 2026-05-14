//! Deploy the three M1 Darwin baskets on Miden testnet.
//!
//! This binary is a structured skeleton. It compiles today against the
//! placeholder Rust types in the workspace; the actual Miden RPC calls
//! become real once `miden-client` is added to the workspace Cargo.toml
//! and the `Client` trait below is replaced with the real
//! `miden_client::Client`.
//!
//! Intended use:
//!
//!     cargo run -p darwin-protocol-account --bin deploy_m1 -- \
//!         --rpc https://rpc.testnet.miden.io \
//!         --operator-key <path-to-falcon-key> \
//!         --pragma-oracle-id <hex-or-bech32>
//!
//! Order of operations (mirrors §5.4 and §6.6 of the M1 spec):
//!
//!   1. Deploy the four custom Darwin asset faucets (dETH, dWBTC,
//!      dUSDT, dDAI).
//!   2. Deploy the darwin-oracle-adapter pointing at the current Pragma
//!      oracle account.
//!   3. For each of the three baskets (DCC, DAG, DCO):
//!      a. Deploy the basket FungibleFaucet with the `agglayer_faucet`
//!         interface.
//!      b. Deploy the Darwin Protocol Account, ownership pointing at the
//!         basket faucet and oracle adapter populated in slot 8.
//!      c. Write the resulting (basket_faucet_id, protocol_account_id)
//!         pair back into the manifest section of `darwin-baskets/state/
//!         testnet.toml` for the SDK to consume.
//!   4. Optionally submit `CONFIG_AGG_BRIDGE` notes to register each
//!      basket faucet with the AggLayer bridge (requires bridge-admin
//!      coordination; not done by this script).

use darwin_protocol_account::{DarwinBasketController, StorageLayout};

fn main() {
    let layout = StorageLayout::default();
    println!("Darwin Protocol Account storage layout (M1 spec §5.2):");
    println!("  version_slot              = {}", layout.version_slot);
    println!(
        "  basket_faucet_id_slot     = {}",
        layout.basket_faucet_id_slot
    );
    println!(
        "  pool_positions_slot       = {}",
        layout.pool_positions_slot
    );
    println!(
        "  target_weights_slot       = {}",
        layout.target_weights_slot
    );
    println!("  last_nav_slot             = {}", layout.last_nav_slot);
    println!(
        "  last_nav_timestamp_slot   = {}",
        layout.last_nav_timestamp_slot
    );
    println!("  pending_ops_slot          = {}", layout.pending_ops_slot);
    println!("  fee_accrual_slot          = {}", layout.fee_accrual_slot);
    println!(
        "  oracle_adapter_id_slot    = {}",
        layout.oracle_adapter_id_slot
    );
    println!(
        "  manifest_version_slot     = {}",
        layout.manifest_version_slot
    );

    println!();
    println!("Baskets to deploy (from darwin-baskets):");
    for basket in darwin_baskets::all_m1() {
        let controller = DarwinBasketController::from_manifest(&basket);
        let manifest = &controller.manifest;
        println!(
            "  {} ({}): {} constituents, drift {} bps, mint/redeem fee {}/{} bps, mgmt {}/y",
            manifest.symbol,
            manifest.name,
            manifest.constituents.len(),
            manifest.rebalancing.drift_threshold_bps,
            manifest.fees.mint_fee_bps,
            manifest.fees.redeem_fee_bps,
            manifest.fees.management_fee_bps_year,
        );
        for c in &manifest.constituents {
            println!(
                "    - {} @ {} bps ({})",
                c.faucet_alias, c.target_weight_bps, c.pragma_pair
            );
        }
    }

    println!();
    println!("This binary is a skeleton. Real deployment lands once the workspace");
    println!("enables miden-base / miden-client / miden-agglayer dependencies.");
}
