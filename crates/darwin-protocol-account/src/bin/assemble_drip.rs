//! Assemble the permissionless drip note script and print its NoteScript root.
//! Placeholder felts — assembly doesn't check them; the real values are
//! templated in by deploy_dispenser at deploy time.

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let root = darwin_protocol_account::drip_note_script(0, 0, 5_000_000)?.root();
    println!("✓ drip_note assembles (p2id + note_tag + wallet linked)");
    println!("  root: {root:?}");
    Ok(())
}
