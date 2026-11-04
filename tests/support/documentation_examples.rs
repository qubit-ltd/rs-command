// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Example {
    pub(crate) id: String,
    pub(crate) mode: String,
    pub(crate) code: String,
}

pub(crate) fn extract_examples(markdown: &str, label: &str) -> Result<Vec<Example>, String> {
    let mut examples = Vec::new();
    let mut marker = None;
    let mut lines = markdown.lines().enumerate().peekable();
    while let Some((line_index, line)) = lines.next() {
        if marker.is_some() && !line.starts_with("```") {
            return Err(format!("{label}: marker must be immediately followed by a Rust fence"));
        }
        let trimmed = line.trim();
        if trimmed.contains("<!-- doc-example:") {
            let value = trimmed
                .strip_prefix("<!-- doc-example:")
                .and_then(|value| value.strip_suffix(" -->"))
                .ok_or_else(|| format!("{label}: malformed marker syntax"))?;
            if marker.is_some() {
                return Err(format!("{label}: marker has no following fence"));
            }
            let (id, mode) = value
                .split_once(':')
                .ok_or_else(|| format!("{label}: malformed marker {value}"))?;
            if id.is_empty() || !matches!(mode, "compile" | "run" | "unix" | "windows" | "no_run" | "fragment") {
                return Err(format!("{label}: unsupported marker {value}"));
            }
            marker = Some((id.to_owned(), mode.to_owned()));
        } else if line.starts_with("```") {
            let fence = line.trim();
            let rust_fence = fence == "```rust" || fence == "```rust,no_run" || fence.starts_with("```rust,");
            if rust_fence && marker.is_none() {
                return Err(format!("{label}:{}: unclassified Rust fence", line_index + 1));
            }
            let Some((id, mode)) = marker.take() else {
                continue;
            };
            if !matches!(fence, "```rust" | "```rust,no_run") {
                return Err(format!("{label}: {id} must precede a supported Rust fence"));
            }
            let no_run = line.trim() == "```rust,no_run";
            if no_run != (mode == "no_run") {
                return Err(format!("{label}: {id} mode does not match fence attributes"));
            }
            let mut code = String::new();
            let mut closed = false;
            for (_, code_line) in lines.by_ref() {
                if code_line.starts_with("```") {
                    closed = true;
                    break;
                }
                if let Some(hidden) = code_line.strip_prefix("# ") {
                    code.push_str(hidden);
                } else if let Some(visible_hash) = code_line.strip_prefix("##") {
                    code.push('#');
                    code.push_str(visible_hash);
                } else {
                    code.push_str(code_line);
                }
                code.push('\n');
            }
            if !closed {
                return Err(format!("{label}: {id} has an unclosed fence"));
            }
            examples.push(Example { id, mode, code });
        }
    }
    if marker.is_some() {
        return Err(format!("{label}: marker has no following Rust fence"));
    }
    let mut ids = std::collections::HashSet::new();
    for example in &examples {
        if !ids.insert(&example.id) {
            return Err(format!("{label}: duplicate marker {}", example.id));
        }
    }
    Ok(examples)
}

pub(crate) fn require_markers(examples: &[Example], expected: &[(&str, &str)], label: &str) -> Result<(), String> {
    for (id, mode) in expected {
        match examples.iter().find(|example| example.id == *id) {
            None => return Err(format!("{label}: missing required marker {id}")),
            Some(example) if example.mode != *mode => {
                return Err(format!(
                    "{label}: marker {id} must use mode {mode}, found {}",
                    example.mode
                ));
            }
            Some(_) => {}
        }
    }
    Ok(())
}

pub(crate) fn check_documents(root: &Path) {
    let documents = [
        ("README.md", &[("quick-start", "compile")][..]),
        ("README.zh_CN.md", &[("quick-start", "compile")][..]),
        (
            "doc/user_guide.md",
            &[
                ("basic-run", "compile"),
                ("unix-output", "unix"),
                ("windows-output", "windows"),
                ("unix-shell", "unix"),
                ("windows-shell", "windows"),
                ("unix-stdin", "unix"),
                ("windows-stdin", "windows"),
                ("structured-command", "compile"),
                ("path-configuration", "compile"),
                ("success-exit-codes", "compile"),
                ("timeout-configuration", "compile"),
                ("application-cancellation", "compile"),
                ("tee-output", "compile"),
                ("sensitive-argument", "compile"),
                ("policy-error", "compile"),
                ("total-output-budget", "no_run"),
            ][..],
        ),
        (
            "doc/user_guide.zh_CN.md",
            &[
                ("basic-run", "compile"),
                ("unix-output", "unix"),
                ("windows-output", "windows"),
                ("unix-shell", "unix"),
                ("windows-shell", "windows"),
                ("unix-stdin", "unix"),
                ("windows-stdin", "windows"),
                ("structured-command", "compile"),
                ("path-configuration", "compile"),
                ("success-exit-codes", "compile"),
                ("timeout-configuration", "compile"),
                ("application-cancellation", "compile"),
                ("tee-output", "compile"),
                ("sensitive-argument", "compile"),
                ("policy-error", "compile"),
                ("total-output-budget", "no_run"),
            ][..],
        ),
    ];
    let mut examples = Vec::new();
    for (relative, expected) in documents {
        let path = root.join(relative);
        let source = fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
        let parsed = extract_examples(&source, relative).unwrap_or_else(|e| panic!("{e}"));
        require_markers(&parsed, expected, relative).unwrap_or_else(|e| panic!("{e}"));
        examples.extend(parsed.into_iter().map(|mut e| {
            e.id = format!("{}__{}", relative.replace(['/', '.'], "_"), e.id);
            e
        }));
    }
    let package_root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let target = PathBuf::from(
        std::env::var_os("CARGO_TARGET_DIR").unwrap_or_else(|| package_root.join("target").into_os_string()),
    );
    let parent = target.join("documentation-example-harness");
    fs::create_dir_all(&parent).expect("create harness parent");
    let harness = tempfile::tempdir_in(parent).expect("create temporary harness");
    fs::create_dir(harness.path().join("tests")).expect("create harness tests directory");
    let dependency_path = package_root.canonicalize().expect("canonicalize package root");
    let manifest = format!(
        "[workspace]\n\n[package]\nname = \"documentation-example-harness\"\nversion = \"0.0.0\"\nedition = \"2024\"\n\n[dependencies]\nqubit-command = {{ path = {:?} }}\n",
        dependency_path.to_string_lossy()
    );
    fs::write(harness.path().join("Cargo.toml"), manifest).expect("write harness manifest");
    let mut source = String::from("#![allow(unused_imports, unused_variables, dead_code, non_snake_case)]\n");
    for example in examples {
        let (cfg, ignored) = match example.mode.as_str() {
            "unix" => ("#[cfg(unix)]\n", false),
            "windows" => ("#[cfg(windows)]\n", false),
            "run" => ("", false),
            "compile" | "no_run" => ("", true),
            "fragment" => continue,
            other => panic!("unsupported mode {other}"),
        };
        source.push_str(cfg);
        source.push_str(&format!("mod {} {{\n", example.id.replace('-', "_")));
        source.push_str("#[test]\n");
        if ignored {
            source.push_str("#[ignore]\n");
        }
        source.push_str("fn example() {\n");
        source.push_str("let result: Result<(), Box<dyn std::error::Error>> = (|| {\n");
        source.push_str(&example.code);
        if !example
            .code
            .trim_end()
            .ends_with("Ok::<(), Box<dyn std::error::Error>>(())")
        {
            source.push_str("Ok(())\n");
        }
        source.push_str("})();\nresult.expect(\"documented example failed\");\n}\n}\n\n");
    }
    fs::write(harness.path().join("tests/documentation_examples.rs"), source).expect("write harness source");
    let status = Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
        .arg("test")
        .arg("--manifest-path")
        .arg(harness.path().join("Cargo.toml"))
        .arg("--target-dir")
        .arg(target.join("documentation-example-target"))
        .status()
        .expect("run temporary Cargo doctest harness");
    assert!(
        status.success(),
        "actual Markdown examples failed Cargo compilation or execution"
    );
}
