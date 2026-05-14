//! Deploy the three M1 Darwin baskets on Miden testnet.
//!
//! Today this binary prints the M1 deployment plan that will be
//! executed once each crate's MASM and Rust component code is filled
//! in. Once the MASM bodies in `asm/controller.masm` and the basket /
//! asset faucets are real, the steps below become live RPC calls to a
//! Miden node.
//!
//! Intended use (post-M1-impl):
//!
//!     cargo run -p darwin-protocol-account --bin deploy_m1 -- \
//!         --rpc https://rpc.testnet.miden.io:57291 \
//!         --operator-key <path-to-falcon-key> \
//!         --pragma-oracle-id <hex-or-bech32>
//!
//! Order of operations (mirrors §5.4 and §6.6 of the M1 spec):
//!
//! 1. Deploy the four custom Darwin asset faucets (dETH, dWBTC, dUSDT,
//!    dDAI).
//! 2. Deploy the darwin-oracle-adapter pointing at the current Pragma
//!    oracle account.
//! 3. For each of the three baskets (DCC, DAG, DCO), deploy the basket
//!    FungibleFaucet with the `agglayer_faucet` interface, then deploy
//!    the Darwin Protocol Account (ownership pointing at the basket
//!    faucet, oracle adapter populated in slot 8), then write the
//!    resulting (basket_faucet_id, protocol_account_id) pair back into
//!    `darwin-baskets/state/testnet.toml` for the SDK to consume.
//! 4. Optionally submit `CONFIG_AGG_BRIDGE` notes to register each
//!    basket faucet with the AggLayer bridge (requires bridge-admin
//!    coordination; not done by this script).

use darwin_protocol_account::miden::{AccountStorageMode, AccountType};
use darwin_protocol_account::{DarwinBasketController, StorageLayout};

fn main() {
    print_layout();
    println!();
    print_planned_accounts();
    println!();
    print_baskets();
    println!();
    test_component_compilation();
    println!();
    print_next_steps();
}

fn test_component_compilation() {
    println!("Stub AccountComponent compilation:");
    for basket in darwin_baskets::all_m1() {
        let controller = DarwinBasketController::from_manifest(&basket);
        match controller.account_component_stub() {
            Ok(component) => {
                println!(
                    "  {} ({}): ✓ compiled, supported_types={:?}",
                    basket.symbol,
                    basket.name,
                    component.supported_types(),
                );
            }
            Err(e) => {
                println!("  {} ({}): ✗ {}", basket.symbol, basket.name, e);
            }
        }
    }
}

fn print_layout() {
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
}

fn print_planned_accounts() {
    println!("Accounts to deploy on Miden testnet:");
    println!(
        "  - 1x Darwin Protocol Account per basket  (type={:?}, storage_mode={:?})",
        AccountType::RegularAccountImmutableCode,
        AccountStorageMode::Private,
    );
    println!(
        "  - 1x basket-token FungibleFaucet per basket  (type={:?}, storage_mode={:?})",
        AccountType::FungibleFaucet,
        AccountStorageMode::Public,
    );
    println!(
        "  - 4x custom asset faucets (dETH, dWBTC, dUSDT, dDAI)  (type={:?}, storage_mode={:?})",
        AccountType::FungibleFaucet,
        AccountStorageMode::Public,
    );
    println!(
        "  - 1x oracle adapter  (type={:?}, storage_mode={:?})",
        AccountType::RegularAccountImmutableCode,
        AccountStorageMode::Public,
    );
}

fn print_baskets() {
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
}

fn print_next_steps() {
    println!("Next steps for the M1 implementation phase:");
    println!("  ✓ MASM procedure bodies in asm/controller_v0_19.masm now use real");
    println!("    felt arithmetic (add / sub / mul) — no longer stubs.");
    println!("  ✓ AccountComponent compiles via miden-objects 0.12's v0.19 Assembler.");
    println!("  ✓ Three controllers deployed on testnet with stub bodies (see");
    println!("    darwin-baskets/state/testnet.toml).");
    println!("  → Pending: redeploy with the new-real-bodies controller once an");
    println!("    integration end-to-end test against `miden-tx` proves the");
    println!("    deposit / redeem stack semantics against a real account context.");
    println!("  → Pending: switch to the v0.23 darwin::* math libraries once");
    println!("    miden-objects releases a build that bundles miden-assembly 0.23,");
    println!("    unlocking storage reads + u64 division in the controller bodies.");
}
