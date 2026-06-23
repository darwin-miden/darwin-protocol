//! Diagnostic: dump the on-stack felts that FungibleAsset's
//! KEY and VALUE words decompose to. Validates the bit-packing
//! we expect when consume notes call into receive_asset.

use miden_client::account::AccountId;
use miden_protocol::asset::FungibleAsset;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let faucet_hex = args
        .get(1)
        .cloned()
        .unwrap_or_else(|| "0xc2c923560dc3cb114ec24ab2291a05".into());
    let amount: u64 = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .unwrap_or(50);

    let faucet = AccountId::from_hex(&faucet_hex)?;
    let fa = FungibleAsset::new(faucet, amount)?;
    let key = fa.to_key_word();
    let val = fa.to_value_word();

    println!("Faucet hex   : {faucet_hex}");
    println!("Faucet suffix: {}", faucet.suffix().as_canonical_u64());
    println!("Faucet prefix: {}", faucet.prefix().as_felt().as_canonical_u64());
    println!("KEY felts (Word index 0..3):");
    println!("  [0] (asset_id.suffix)         = {}", key[0].as_canonical_u64());
    println!("  [1] (asset_id.prefix)         = {}", key[1].as_canonical_u64());
    println!("  [2] (faucet_suffix|metadata)  = {}", key[2].as_canonical_u64());
    println!("  [3] (faucet_prefix)           = {}", key[3].as_canonical_u64());
    println!("VAL felts:");
    println!("  [0] (amount)                  = {}", val[0].as_canonical_u64());
    println!("  [1]                           = {}", val[1].as_canonical_u64());
    println!("  [2]                           = {}", val[2].as_canonical_u64());
    println!("  [3]                           = {}", val[3].as_canonical_u64());

    Ok(())
}
