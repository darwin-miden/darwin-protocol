// Build script for darwin-protocol-account.
//
// Once `miden-assembly` is enabled in the workspace Cargo.toml, the
// block below will assemble `asm/controller.masm` into a `.masl`
// artefact at build time and emit it to OUT_DIR for `include_bytes!`
// pickup in `src/lib.rs`. Until then this script is a no-op that just
// re-runs when the MASM source changes — enough to give Cargo a hint
// during the placeholder phase.

fn main() {
    println!("cargo:rerun-if-changed=asm/controller.masm");

    // --- Enable once `miden-assembly` is a dependency: ---
    //
    // use miden_assembly::Assembler;
    // use std::path::PathBuf;
    //
    // let asm_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("asm");
    // let out_dir = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    //
    // let assembler = Assembler::default()
    //     .with_library(miden_stdlib::StdLibrary::default())
    //     .expect("stdlib loads");
    //
    // let library = assembler
    //     .assemble_library([asm_dir.join("controller.masm")])
    //     .expect("controller.masm assembles");
    //
    // library
    //     .write_to_file(out_dir.join("controller.masl"))
    //     .expect("write controller.masl");
}
