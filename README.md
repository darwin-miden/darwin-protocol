# darwin-protocol

Core Darwin Protocol code on Miden: the Darwin Protocol Account, the basket-token faucets, the custom testnet asset faucets, and the note scripts (`DepositNote`, `RedeemNote`).

See [`darwin-docs/architecture-spec.md`](https://github.com/darwin-miden/darwin-docs/blob/main/docs/architecture-spec.md) for the full specification.

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

Live on Miden testnet. The Rust crates compile, unit tests pass, and
the protocol's user-facing flows (atomic deposit, redeem, Flow B
rebalance trigger, Flow C symmetric redeem) are exercised end-to-end
on the public Miden testnet against the v6 fee-routing controller
(`0x2a3ea0a268d97b80497d6a966e3141`). See
[`darwin-docs/status.md`](https://github.com/darwin-miden/darwin-docs/blob/main/docs/status.md)
for the live tx hashes per flow.

The MASM bodies live under `crates/*/asm/` as canonical source.
Controller variants (v3/v4/v5/v6) are built as Miden packages by
the `build_v*_controller` binaries in
`crates/darwin-protocol-account/src/bin/`; running e.g.
`cargo run --release --bin build_v6_fee_routing_controller` emits a
`.masp` artifact the deployment scripts then push to the testnet.

## Build

```bash
cargo build --release           # core crates + atomic-note assembly
cargo test --lib                # workspace unit tests

# live Pragma read (requires the pragma-live feature):
cargo run --release --features pragma-live \
  -p darwin-protocol-account --bin oracle_query_real
```

## License

MIT.
