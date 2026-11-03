// Bind syntax continuations to the reference data and code in this build.
use sha2::{Digest, Sha256};
use std::fs;

fn main() {
    const FILES: &[&str] = &[
        "src/engine/catalog/syntax_data.json",
        "src/engine/catalog/syntax.rs",
        "src/engine/catalog/mod.rs",
        "src/engine/catalog/catalog_data.rs",
        "src/engine/catalog/option_names.rs",
        "src/mcp/canonical.rs",
        "src/mcp/result.rs",
    ];
    let mut hash = Sha256::new();
    for path in FILES {
        println!("cargo:rerun-if-changed={path}");
        let bytes = fs::read(path).unwrap_or_else(|err| panic!("Read {path}: {err}"));
        hash.update((path.len() as u64).to_le_bytes());
        hash.update(path.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    println!(
        "cargo:rustc-env=DYGNOSIS_SYNTAX_REVISION={:x}",
        hash.finalize()
    );
}
