//! Native writer for the on-chain encrypted backup — the Mac-relay path.
//! The browser encrypts the account file locally (AES key derived from the
//! user's MetaMask signature, never shared) and POSTs ONLY the ciphertext here;
//! this bin packs it into 28-byte Words and writes them into the public NoAuth
//! controller's slot-10 StorageMap, natively (fast proving, no browser freeze).
//! Confidentiality is preserved: only opaque ciphertext + public ids are seen.
//!
//! Usage: backup_write <controller_hex> <suffix_u64> <prefix_u64>   (bytes on stdin)
//! Prints: {"ok":true,"nWords":N,"byteLen":B} or {"error":"..."}
//!
//!   MIDEN_NETWORK=testnet HOME=/Users/eden/data/darwin/.relay-miden-testnet \
//!   backup_write 0x2cc265c53378fb3171eaf12e03c644 <suf> <pre> < ciphertext.bin

use std::io::Read;
use std::path::PathBuf;
use std::sync::Arc;

use miden_client::account::AccountId;
use miden_client::builder::ClientBuilder;
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client_sqlite_store::SqliteStore;

// Must match src/lib/onchainBackup.ts and backup_read.rs.
const MAGIC: &str = "15720690719117082606"; // 0xda2b1cead0c0ffee
const META_INDEX: usize = 4294967295; // 0xffffffff
const PER_TX: usize = 48; // set_map_item calls per tx (native prover handles it)

/// Write value [v0,v1,v2,v3] under key [index, MAGIC, prefix, suffix]. Push the
/// value reversed (v0 on top) ⇒ stored word = [v0,v1,v2,v3]; matches backup_read.
fn write_one(v: [u64; 4], index: usize, suffix: &str, prefix: &str) -> String {
    format!(
        "  push.{} push.{} push.{} push.{}\n  push.{suffix} push.{prefix} push.{MAGIC} push.{index}\n  call.{SET}\n  dropw\n",
        v[3], v[2], v[1], v[0],
        SET = "0xea652ac9aa1b6ee468da0845b52008ffa4639d112f356534ba608bc00d7b6f5f",
    )
}

/// Pack bytes into Words: 7 bytes/felt (LE), 4 felts/word. Mirrors packBytesToWords.
fn pack_words(bytes: &[u8]) -> Vec<[u64; 4]> {
    let mut words = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let mut w = [0u64; 4];
        for (f, slot) in w.iter_mut().enumerate() {
            let mut felt = 0u64;
            for b in 0..7 {
                let idx = i + f * 7 + b;
                if idx < bytes.len() {
                    felt |= (bytes[idx] as u64) << (8 * b);
                }
            }
            *slot = felt;
        }
        words.push(w);
        i += 28;
    }
    words
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    match run().await {
        Ok((n_words, byte_len)) => {
            println!("{{\"ok\":true,\"nWords\":{n_words},\"byteLen\":{byte_len}}}");
        }
        Err(e) => {
            let msg = e.to_string().replace('"', "'").replace('\n', " ");
            println!("{{\"error\":\"{}\"}}", &msg[..msg.len().min(300)]);
            std::process::exit(1);
        }
    }
}

async fn run() -> Result<(usize, usize), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        return Err("usage: backup_write <controller_hex> <suffix> <prefix> (bytes on stdin)".into());
    }
    let controller = AccountId::from_hex(&args[1])?;
    let suffix = args[2].clone();
    let prefix = args[3].clone();

    // Read the ciphertext from stdin.
    let mut bytes = Vec::new();
    std::io::stdin().read_to_end(&mut bytes)?;
    if bytes.is_empty() {
        return Err("empty ciphertext on stdin".into());
    }
    let byte_len = bytes.len();
    let words = pack_words(&bytes);

    let home = std::env::var("HOME")?;
    let store = SqliteStore::new(PathBuf::from(format!("{home}/.miden/store.sqlite3"))).await?;
    let mut client = ClientBuilder::<FilesystemKeyStore>::new()
        .grpc_client(&darwin_protocol_account::miden_endpoint(), None)
        .store(Arc::new(store))
        .filesystem_keystore(PathBuf::from(format!("{home}/.miden/keystore")))?
        .build()
        .await?;
    client.sync_state().await?;

    // Build scripts: chunk batches, then meta LAST (partial write never looks complete).
    let mut scripts: Vec<String> = Vec::new();
    let mut i = 0;
    while i < words.len() {
        let mut src = String::from("use miden::core::sys\n\nbegin\n");
        for j in i..(i + PER_TX).min(words.len()) {
            src.push_str(&write_one(words[j], j, &suffix, &prefix));
        }
        src.push_str("  exec.sys::truncate_stack\nend\n");
        scripts.push(src);
        i += PER_TX;
    }
    let mut meta = String::from("use miden::core::sys\n\nbegin\n");
    meta.push_str(&write_one(
        [byte_len as u64, words.len() as u64, 0, 0],
        META_INDEX,
        &suffix,
        &prefix,
    ));
    meta.push_str("  exec.sys::truncate_stack\nend\n");
    scripts.push(meta);

    for src in &scripts {
        let tx = client.code_builder().compile_tx_script(src).map_err(|e| format!("compile: {e}"))?;
        let req = TransactionRequestBuilder::new().custom_script(tx).build().map_err(|e| format!("build: {e}"))?;
        let r = client.execute_transaction(controller, req).await?;
        let prover = client.prover();
        let p = client.prove_transaction_with(&r, prover).await?;
        let h = client.submit_proven_transaction(p, &r).await?;
        client.apply_transaction(&r, h).await?;
        let _ = h;
    }
    Ok((words.len(), byte_len))
}
