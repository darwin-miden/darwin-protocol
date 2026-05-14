//! Build a `.masp` for the Darwin mock Pragma-style oracle.
//!
//! Compiles `darwin-oracle-adapter/asm/mock_oracle.masm` against
//! `miden_protocol::TransactionKernel::assembler()` (so
//! `miden::protocol::native_account` resolves), wraps in a Package
//! with `AccountComponentMetadata`, writes `.masp`.
//!
//! Deploy with:
//!     miden client new-account \
//!         --account-type regular-account-immutable-code \
//!         --packages /tmp/darwin-mock-oracle.masp \
//!         --storage-mode public \
//!         --deploy
//!
//! Usage:
//!     cargo run -p darwin-protocol-account --bin build_mock_oracle_package -- \
//!         --out /tmp/darwin-mock-oracle.masp

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::serde::Serializable;
use miden_assembly::{DefaultSourceManager, Path};
use miden_mast_package::{Package, PackageId, Section, SectionId, TargetType, Version};
use miden_protocol::account::component::AccountComponentMetadata;
use miden_protocol::account::AccountType;

const ORACLE_NAMESPACE: &str = "darwin::mock_oracle";
const ORACLE_SOURCE: &str = include_str!("../../../../../darwin-oracle-adapter/asm/mock_oracle.masm");

fn parse_args() -> PathBuf {
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--out" || a == "-o" {
            out = Some(PathBuf::from(args.next().expect("--out value")));
        }
    }
    out.unwrap_or_else(|| PathBuf::from("darwin-mock-oracle.masp"))
}

fn main() {
    let out_path = parse_args();

    let sm: Arc<dyn miden_assembly::SourceManager> = Arc::new(DefaultSourceManager::default());
    let module = Module::parser(ModuleKind::Library)
        .parse_str(Path::new(ORACLE_NAMESPACE), ORACLE_SOURCE, sm)
        .expect("oracle source parses");

    let lib = miden_protocol::transaction::TransactionKernel::assembler()
        .assemble_library([module])
        .expect("oracle library assembles");

    println!("Mock oracle procedures (MAST roots):");
    for mi in lib.module_infos() {
        for (_, pi) in mi.procedures() {
            let bytes: Vec<u8> = pi
                .digest
                .as_elements()
                .iter()
                .flat_map(|f| f.as_canonical_u64().to_le_bytes())
                .collect();
            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            println!("  {}::{:<20} call.0x{}", mi.path(), pi.name, hex);
        }
    }

    let metadata = AccountComponentMetadata::new(
        "darwin-mock-oracle",
        [AccountType::RegularAccountImmutableCode],
    )
    .with_description(
        "Mock Pragma-style oracle. Mirrors get_median / get_entry. \
         Used to demonstrate cross-account oracle calls from Darwin \
         on Miden testnet, independent of Pragma's build pipeline.",
    );

    let mut package = Package::from_library(
        PackageId::from("darwin-mock-oracle"),
        Version::new(0, 1, 0),
        TargetType::AccountComponent,
        lib,
        std::iter::empty(),
    );
    package
        .sections
        .push(Section::new(SectionId::ACCOUNT_COMPONENT_METADATA, metadata.to_bytes()));

    package
        .write_to_file(&out_path)
        .unwrap_or_else(|e| panic!("write {}: {}", out_path.display(), e));

    println!("Wrote {} ({} bytes)", out_path.display(),
        std::fs::metadata(&out_path).map(|m| m.len()).unwrap_or(0));
    println!();
    println!("Deploy with:");
    println!("  miden client new-account \\");
    println!("    --account-type regular-account-immutable-code \\");
    println!("    --packages {} \\", out_path.display());
    println!("    --storage-mode public \\");
    println!("    --deploy");
}
