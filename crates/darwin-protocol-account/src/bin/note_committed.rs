//! Reports whether a note id is committed on-chain by querying the node RPC.
//! Prints "committed" or "pending" (and exits 0 either way). Used by the faucet's
//! /api/note-status so the browser can poll for a payout's readiness WITHOUT a
//! wallet prompt (the in-browser RpcClient path hits WASM type-marshalling bugs).

use miden_client::note::NoteId;
use miden_client::rpc::{GrpcClient, NodeRpcClient};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(run())
}

async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 2 {
        return Err(format!("usage: {} <note_id_hex>", args[0]).into());
    }
    let note_id = NoteId::try_from_hex(&args[1])?;
    let endpoint = darwin_protocol_account::miden_endpoint();
    let rpc = GrpcClient::new(&endpoint, 10_000);
    match rpc.get_note_by_id(note_id).await {
        Ok(_) => println!("committed"),
        Err(_) => println!("pending"),
    }
    Ok(())
}
