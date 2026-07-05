//! Prove that a fresh keystore-less client can WRITE slot-10 on
//! v8-noauth via a tx script. This matches what the browser does in
//! TrustlessDepositPanel step 4.
//!
//! Success criterion: after the tx, `active_account::get_map_item` on
//! slot 10 with the fake user_basket_key returns the amount word we
//! set.

use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::transaction::{TransactionKernel, TransactionScript};

const V8_NOAUTH: &str = "0x2cc265c53378fb3171eaf12e03c644";
const SET_USER_POSITION_MAST: &str =
    "0xea652ac9aa1b6ee468da0845b52008ffa4639d112f356534ba608bc00d7b6f5f";

// Fake EVM addr-derived felts + amount for the write.
const USER_ID_SUFFIX: u64 = 0x_1122_3344_5566_7788;
const USER_ID_PREFIX: u64 = 0x_00aa_bbcc_dd00_0000;
const AMOUNT: u64 = 12345;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let home = std::env::var("HOME")?;
    let store_path = PathBuf::from(format!("{home}/.miden/store.sqlite3"));
    let keystore_path = PathBuf::from(format!("{home}/.miden/keystore"));
    std::fs::create_dir_all(&keystore_path)?;

    println!("Connecting to Miden testnet…");
    let store = SqliteStore::new(store_path).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(keystore_path)?
        .build()
        .await?;

    let v8 = AccountId::from_hex(V8_NOAUTH)?;
    println!("Importing v8-noauth {v8} (post-initial-commit)…");
    client.import_account_by_id(v8).await?;
    client.sync_state().await?;

    // Build the tx script: push (KEY, VALUE) and call set_user_position.
    // Stack top-down convention: [KEY_WORD, VALUE_WORD] before the call.
    let src = format!(
        r#"
use miden::core::sys

begin
    # VALUE word first (deepest on stack):
    #   [0, 0, 0, amount]
    push.{amount} push.0 push.0 push.0

    # KEY word on top:
    #   [0, 0, user_prefix, user_suffix]
    push.{suffix} push.{prefix} push.0 push.0

    call.{mast}

    exec.sys::truncate_stack
end
"#,
        amount = AMOUNT,
        suffix = USER_ID_SUFFIX,
        prefix = USER_ID_PREFIX,
        mast = SET_USER_POSITION_MAST,
    );
    println!("Tx script MASM:\n{src}");

    let program = TransactionKernel::assembler().assemble_program(&src)?;
    let tx_script = TransactionScript::new(program);

    let req = TransactionRequestBuilder::new()
        .custom_script(tx_script)
        .build()?;

    println!("=== execute_transaction(v8, tx_script) — no key in keystore ===");
    let res = client.execute_transaction(v8, req).await?;
    let tx_id = res.executed_transaction().id();
    println!("executed tx_id={tx_id}");

    let prover = client.prover();
    let proven = client.prove_transaction_with(&res, prover).await?;
    let height = client.submit_proven_transaction(proven, &res).await?;
    client.apply_transaction(&res, height).await?;
    println!("submitted at block {height}");

    Ok(())
}
