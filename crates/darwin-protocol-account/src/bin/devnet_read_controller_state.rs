//! Dump the v7 controller's storage from Devnet — slots view +
//! per-basket map entries for slots 3 (target_weights) and 4 (fees).

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::{AccountId, StorageMapKey, StorageSlotContent};
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client_sqlite_store::SqliteStore;

const CONTROLLER_HEX: &str = "0x2388eaea4ce45331214b871755e7b5";
const DCC_FAUCET_HEX: &str = "0x536e8b33e2e10d915bd466faa64099";
const DAG_FAUCET_HEX: &str = "0x6c4f5da5061c6f312e99327a5b36d3";
const DCO_FAUCET_HEX: &str = "0xf1be7df227291a714c62658a3bcd18";

fn basket_key(faucet: AccountId) -> miden_client::Word {
    miden_client::Word::try_from(
        [
            miden_client::Felt::new(faucet.suffix().as_canonical_u64()).unwrap(),
            miden_client::Felt::new(faucet.prefix().as_felt().as_canonical_u64()).unwrap(),
            miden_client::Felt::new(0).unwrap(),
            miden_client::Felt::new(0).unwrap(),
        ]
        .as_slice(),
    )
    .unwrap()
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path: PathBuf = format!("{home}/.miden/store.sqlite3").into();
    let keystore_path: PathBuf = format!("{home}/.miden/keystore").into();

    let endpoint = darwin_protocol_account::miden_endpoint();
    println!("Connecting to Miden ({endpoint:?})…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&endpoint, None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;
    client.sync_state().await?;

    let controller = AccountId::from_hex(CONTROLLER_HEX)?;
    let storage = client.get_account_storage(controller).await?;

    println!();
    println!("══ v7 controller storage — {CONTROLLER_HEX} ══");
    println!("  num_slots: {}", storage.num_slots());

    let target_weights_map = None;
    let fees_map = None;
    let mut tw_ref: Option<&miden_client::account::StorageMap> = target_weights_map;
    let mut fees_ref: Option<&miden_client::account::StorageMap> = fees_map;

    for (idx, slot) in storage.slots().iter().enumerate() {
        match slot.content() {
            StorageSlotContent::Value(w) => {
                println!(
                    "  slot {idx:>2} (Value): [{}]",
                    w.iter()
                        .map(|f| f.as_canonical_u64().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            StorageSlotContent::Map(m) => {
                println!("  slot {idx:>2} (Map, {} entries)", m.entries().count());
                // The v6 component prepends 4 system slots, so the spec's
                // slot 3 (target_weights) lives at index 7, slot 4 (fees) at
                // index 8, slot 10 (user_positions) at 14 — outside 0..14
                // when num_slots=14, so it's currently inaccessible. The
                // M3 deploy_v6_init wrote into 7/8 (and slot 12 for
                // fee_recipient at index 11+1 = 12 because of the offset).
                // Live Devnet probe confirms: slot 7 ≡ fees map, slot 8
                // ≡ target_weights map. The proc names line up with the
                // raw values [mint_bps, redeem_bps, mgmt_bps] and
                // [w_btc, w_eth, w_other] respectively.
                if idx == 7 {
                    fees_ref = Some(m);
                }
                if idx == 8 {
                    tw_ref = Some(m);
                }
            }
        }
    }

    println!();
    println!("══ Map lookups for DCC/DAG/DCO ══");
    for (label, hex) in [
        ("DCC", DCC_FAUCET_HEX),
        ("DAG", DAG_FAUCET_HEX),
        ("DCO", DCO_FAUCET_HEX),
    ] {
        let basket = AccountId::from_hex(hex)?;
        let key = StorageMapKey::from_raw(basket_key(basket));
        if let Some(m) = tw_ref {
            let v = m.get(&key);
            println!(
                "  {label} target_weights: [{}]",
                v.iter()
                    .map(|f| f.as_canonical_u64().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        if let Some(m) = fees_ref {
            let v = m.get(&key);
            println!(
                "  {label} fees:           [{}]",
                v.iter()
                    .map(|f| f.as_canonical_u64().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }

    Ok(())
}
