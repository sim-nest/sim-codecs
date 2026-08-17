// conformance: the retained classfile recipe executes through the public inspection surface.

use std::{fs, path::PathBuf};

use sim_codec_classfile::inspect_classfile;
use sim_kernel::{CodecId, Expr, Symbol};

#[test]
fn retained_classfile_recipe_executes_without_a_jvm() {
    let crate_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let recipe_root = crate_root.join("recipes/01-inspection/retained-classfile");
    let manifest = fs::read_to_string(recipe_root.join("recipe.toml")).unwrap();
    let recipe = manifest.parse::<toml::Table>().unwrap();
    let setup = recipe["setup"].as_str().unwrap();
    let bytes = fs::read(recipe_root.join(setup)).unwrap();

    let projection = inspect_classfile(CodecId(73), bytes.clone(), 4_096).unwrap();
    let Expr::Extension { tag, payload } = projection else {
        panic!("classfile recipe did not produce a retained projection")
    };
    assert_eq!(tag, Symbol::qualified("classfile", "Classfile"));
    let Expr::Map(entries) = payload.as_ref() else {
        panic!("classfile recipe projection is not browseable")
    };
    assert!(entries.iter().any(|(key, value)| {
        key == &Expr::Symbol(Symbol::new("bytes")) && value == &Expr::Bytes(bytes.clone())
    }));
}
