#[path = "build_support/schema_oracle.rs"]
mod schema_oracle;

fn main() {
    sim_cookbook_build::write_embed("recipes").expect("embed cookbook recipes");
    let manifest = std::path::PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let output = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    schema_oracle::validate_and_generate(&manifest, &output)
        .expect("validate pinned MCP schema coverage oracle");
    println!("cargo:rerun-if-changed=fixtures/mcp");
}
