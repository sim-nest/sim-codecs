//! Source file-size gate for the sim-codecs workspace.

use std::fs;
use std::path::{Path, PathBuf};

const GENERAL_SOFT_LIMIT: usize = 500;
const GENERAL_HARD_LIMIT: usize = 700;
const ENTRYPOINT_SOFT_LIMIT: usize = 150;
const ENTRYPOINT_HARD_LIMIT: usize = 250;

pub fn run(args: &[String]) -> Result<(), String> {
    if args.len() != 2 {
        let program = args.first().map(String::as_str).unwrap_or("xtask");
        return Err(format!("usage: {program} check-file-sizes"));
    }

    let root = std::env::current_dir().map_err(|err| format!("current dir: {err}"))?;
    let mut files = Vec::new();
    collect_rs_files(&root.join("crates"), &mut files)?;
    collect_rs_files(&root.join("xtask").join("src"), &mut files)?;
    files.sort();

    let mut warnings = Vec::new();
    let mut failures = Vec::new();
    for path in &files {
        let lines = count_lines(path)?;
        let limits = limits_for(path);
        let rel = relative_path(&root, path);
        if lines > limits.hard {
            failures.push(format!(
                "{rel}: {lines} line(s), hard limit is {}",
                limits.hard
            ));
        } else if lines > limits.soft {
            warnings.push(format!(
                "{rel}: {lines} line(s), soft target is {}",
                limits.soft
            ));
        }
    }

    for warning in &warnings {
        println!("warning: {warning}");
    }
    if !failures.is_empty() {
        for failure in &failures {
            eprintln!("error: {failure}");
        }
        return Err(format!(
            "check-file-sizes: {} file(s) exceed hard limits",
            failures.len()
        ));
    }

    println!(
        "check-file-sizes: OK ({} Rust file(s), {} soft warning(s), 0 hard failure(s))",
        files.len(),
        warnings.len()
    );
    Ok(())
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir).map_err(|err| format!("read {}: {err}", dir.display()))? {
        let entry = entry.map_err(|err| format!("read {} entry: {err}", dir.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|err| format!("read {} file type: {err}", path.display()))?;
        if file_type.is_dir() {
            collect_rs_files(&path, files)?;
        } else if path.extension().is_some_and(|extension| extension == "rs") {
            files.push(path);
        }
    }
    Ok(())
}

fn count_lines(path: &Path) -> Result<usize, String> {
    let content =
        fs::read_to_string(path).map_err(|err| format!("read {}: {err}", path.display()))?;
    Ok(content.lines().count())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Limits {
    soft: usize,
    hard: usize,
}

fn limits_for(path: &Path) -> Limits {
    match path.file_name().and_then(|name| name.to_str()) {
        Some("lib.rs" | "main.rs" | "mod.rs") => Limits {
            soft: ENTRYPOINT_SOFT_LIMIT,
            hard: ENTRYPOINT_HARD_LIMIT,
        },
        _ => Limits {
            soft: GENERAL_SOFT_LIMIT,
            hard: GENERAL_HARD_LIMIT,
        },
    }
}

fn relative_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .display()
        .to_string()
}
