// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandRunner;

const CARGO_TOML: &str = include_str!("../Cargo.toml");
const README_EN: &str = include_str!("../README.md");
const README_ZH: &str = include_str!("../README.zh_CN.md");
const GUIDE_EN: &str = include_str!("../doc/user_guide.md");
const GUIDE_ZH: &str = include_str!("../doc/user_guide.zh_CN.md");

#[test]
fn documentation_install_constraints_match_the_crate_compatibility_line() {
    let version = CARGO_TOML
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|line| line.strip_suffix('\"'))
        .expect("Cargo.toml should declare a package version");
    let mut components = version.split('.');
    let major = components.next().expect("version should have a major component");
    let minor = components.next().expect("version should have a minor component");
    let compatibility_line = format!("{major}.{minor}");
    let dependency = format!("qubit-command = \"{compatibility_line}\"");

    for (name, contents) in [
        ("README.md", README_EN),
        ("README.zh_CN.md", README_ZH),
        ("doc/user_guide.md", GUIDE_EN),
        ("doc/user_guide.zh_CN.md", GUIDE_ZH),
    ] {
        assert!(contents.contains(&dependency), "{name} should use {dependency}");
    }

    assert!(GUIDE_EN.contains(&format!("qubit-command` {version}")));
    assert!(GUIDE_ZH.contains(&format!("qubit-command` {version}")));
}

#[test]
fn documented_total_output_budget_api_type_checks() {
    // Keep the guide's API usage compiled without spawning a platform-specific
    // process.
    let example = || {
        let result = CommandRunner::without_timeout()
            .max_total_output_bytes(1024)
            .run(Command::new("example"));
        if let Err(error) = result
            && error.kind() == CommandErrorKind::OutputLimitExceeded
        {
            let _ = error.captured_output();
        }
    };
    let _ = example;
}
