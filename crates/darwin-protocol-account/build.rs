// Build script for darwin-protocol-account.
//
// Assembles every MASM module under `asm/lib/` into a single
// `darwin::*` library and writes the result to `$OUT_DIR/darwin.masl`
// so the crate can `include_bytes!` it and load it via
// `Library::read_from_bytes` at runtime.
//
// Build order respects the dependency graph documented in
// `asm/lib/flow.masm` (flow depends on nav/mint/fees/redeem). We
// assemble the four primitive modules into a "primitives" library
// first, then assemble flow against that library.

use std::path::PathBuf;
use std::sync::Arc;

use miden_assembly::ast::{Module, ModuleKind};
use miden_assembly::Assembler;
use miden_assembly::{DefaultSourceManager, ModuleParser, Path, SourceManager};

const PRIMITIVES: &[(&str, &str)] = &[
    ("darwin::math", "asm/lib/math.masm"),
    ("darwin::nav", "asm/lib/nav.masm"),
    ("darwin::mint", "asm/lib/mint.masm"),
    ("darwin::fees", "asm/lib/fees.masm"),
    ("darwin::redeem", "asm/lib/redeem.masm"),
];

const FLOW_NAMESPACE: &str = "darwin::flow";
const FLOW_PATH: &str = "asm/lib/flow.masm";

fn main() {
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").expect("OUT_DIR set"));

    // Cargo rerun triggers — one per MASM source.
    for (_, rel) in PRIMITIVES {
        println!("cargo:rerun-if-changed={rel}");
    }
    println!("cargo:rerun-if-changed={FLOW_PATH}");

    let source_manager: Arc<dyn SourceManager> = Arc::new(DefaultSourceManager::default());

    // Parse each primitive module under its own `darwin::*` namespace.
    let primitive_modules: Vec<Box<Module>> = PRIMITIVES
        .iter()
        .map(|(namespace, rel)| {
            let path = manifest_dir.join(rel);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
            let mut parser = ModuleParser::new(ModuleKind::Library);
            parser
                .parse_str(Path::new(*namespace), source, source_manager.clone())
                .unwrap_or_else(|e| panic!("parse {}: {}", namespace, e))
        })
        .collect();

    // `darwin::math` imports `miden::core::math::u64` — attach the core
    // library so the assembler can resolve that path.
    let core_library = miden_core_lib::CoreLibrary::default();

    let primitives_lib = Assembler::default()
        .with_static_library(core_library.as_ref())
        .expect("core library attaches to primitives assembler")
        .assemble_library(primitive_modules)
        .expect("primitive modules assemble into a library");

    let primitives_path = out_dir.join("darwin-primitives.masl");
    primitives_lib
        .write_to_file(&primitives_path)
        .unwrap_or_else(|e| panic!("write {}: {}", primitives_path.display(), e));

    // Parse and assemble the flow module against the primitives lib.
    let flow_source =
        std::fs::read_to_string(manifest_dir.join(FLOW_PATH)).expect("flow source readable");
    let mut parser = ModuleParser::new(ModuleKind::Library);
    let flow_module = parser
        .parse_str(Path::new(FLOW_NAMESPACE), flow_source, source_manager)
        .expect("flow source parses");

    let flow_lib = Assembler::default()
        .with_static_library(core_library.as_ref())
        .expect("core library attaches to flow assembler")
        .with_static_library(&primitives_lib)
        .expect("primitives attaches to flow assembler")
        .assemble_library([flow_module])
        .expect("flow library assembles");

    let flow_path = out_dir.join("darwin-flow.masl");
    flow_lib
        .write_to_file(&flow_path)
        .unwrap_or_else(|e| panic!("write {}: {}", flow_path.display(), e));
}
