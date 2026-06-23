//! Decode a bech32 Miden address and dump every field.
//! Useful to compare what the v0.15 client sees vs what a faucet sees.

use miden_client::account::{AccountId, Address, NetworkId};
use miden_protocol::address::AddressId;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return Err("usage: decode_address <bech32_or_hex>".into());
    }
    let input = &args[1];

    if input.starts_with("0x") {
        let id = AccountId::from_hex(input)?;
        println!("HEX input");
        println!("  id (hex)     : {}", id.to_hex());
        println!("  suffix       : {}", id.suffix().as_canonical_u64());
        println!("  prefix       : {}", id.prefix().as_felt().as_canonical_u64());
        println!("  account_type : {:?}", id.account_type());
        return Ok(());
    }

    let (network_id, address) = Address::decode(input)?;
    println!("BECH32 input");
    println!("  network_id   : {network_id:?}");
    let id = match address.id() {
        AddressId::AccountId(aid) => aid,
        _ => return Err("non-AccountId AddressId variant".into()),
    };
    println!("  account_id   : {}", id.to_hex());
    println!("  suffix       : {}", id.suffix().as_canonical_u64());
    println!("  prefix       : {}", id.prefix().as_felt().as_canonical_u64());
    println!("  account_type : {:?}", id.account_type());
    println!("  routing      : interface={:?} tag_len={}",
             address.interface(),
             address.note_tag_len());
    Ok(())
}
