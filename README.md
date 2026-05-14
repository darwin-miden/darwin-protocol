# darwin-protocol

Core Darwin Protocol code on Miden: the Darwin Protocol Account, the basket-token faucets, the custom testnet asset faucets, and the note scripts (`DepositNote`, `RedeemNote`).

See [`darwin-docs/m1-architecture-spec.md`](https://github.com/darwin-miden/darwin-docs/blob/main/docs/m1-architecture-spec.md) for the full specification.

## Workspace layout

```
darwin-protocol/
├── Cargo.toml                                  # workspace
├── crates/
│   ├── darwin-protocol-account/                # §5 — Darwin Protocol Account
│   │   ├── src/                                # storage layout, component spec
│   │   └── asm/controller.masm                 # DarwinBasketController MASM
│   ├── darwin-basket-faucet/                   # §6.6 — DCC / DAG / DCO faucets
│   │   ├── src/                                # synthetic origin address, builders
│   │   └── asm/faucet.masm                     # FungibleFaucet + agglayer_faucet
│   ├── darwin-asset-faucets/                   # §4.2 — dETH, dWBTC, dUSDT, dDAI
│   │   └── src/
│   └── darwin-notes/                           # §7 — DepositNote, RedeemNote
│       ├── src/
│       └── asm/{deposit_note,redeem_note}.masm
```

## Status

This is a **scaffold**. The Rust crates compile and pass their unit tests today. The MASM procedure bodies are intentionally stubbed pending the Miden v0.14 toolchain integration — they document the contract surface and contain `# TODO` markers for every body still to be implemented.

The workspace `Cargo.toml` includes commented-out git dependencies for `miden-base`, `miden-client`, `miden-assembly`, `miden-agglayer` (all pinned to the `next` branch of `0xMiden/protocol`). Uncomment them in step with installing the Miden toolchain locally — see the Getting Started guide in `darwin-docs`.

## Build

```bash
cargo build
cargo test
```

Once the Miden toolchain is enabled, the MASM in `crates/*/asm/` will be compiled at build time via the `miden-assembly` crate's `build.rs` hook (to be added).

## License

MIT.
