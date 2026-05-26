//! Compute the (suffix, prefix) felt pair that each v5 storage slot
//! name hashes to. The MASM body uses these as literal constants in
//! its `push.<suffix> push.<prefix>` storage-map addressing — slot
//! IDs in Miden are `hash_string_to_word(name)[0..2]`, not the array
//! index, so we have to bake the actual hash values into the script.

use miden_protocol::account::StorageSlotName;

fn main() {
    for i in 0..=10 {
        let name = format!("darwin::slot_{i}");
        let slot = StorageSlotName::new(name.clone()).expect("slot name");
        let id = slot.id();
        println!(
            "  slot {i:2} \"{}\": suffix={} prefix={}",
            name,
            id.suffix().as_canonical_u64(),
            id.prefix().as_canonical_u64(),
        );
    }
}
