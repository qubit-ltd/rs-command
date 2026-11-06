// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
const CARGO_TOML: &str = include_str!("../Cargo.toml");
const README_EN: &str = include_str!("../README.md");
const README_ZH: &str = include_str!("../README.zh_CN.md");
const GUIDE_EN: &str = include_str!("../doc/user_guide.md");
const GUIDE_ZH: &str = include_str!("../doc/user_guide.zh_CN.md");

fn compatibility_line(version: &str) -> String {
    let mut components = version.split('.');
    let major = components.next().expect("version should have a major component");
    let minor = components.next().expect("version should have a minor component");
    format!("{major}.{minor}")
}

fn declares_version(contents: &str, expected: &str) -> bool {
    contents.lines().any(|line| {
        line.split_once("qubit-command` ")
            .and_then(|(_, rest)| rest.split_whitespace().next())
            .map(|token| token.trim_end_matches(['.', ',', '，', '。']))
            == Some(expected)
    })
}

#[test]
fn test_compatibility_line_uses_only_major_and_minor_components() {
    assert_eq!(compatibility_line("0.11.0"), "0.11");
    assert_eq!(compatibility_line("0.11.1"), "0.11");
    assert_eq!(compatibility_line("0.12.0"), "0.12");
}

#[test]
fn test_version_declaration_matches_a_complete_major_minor_token() {
    assert!(declares_version("This guide describes `qubit-command` 0.11.", "0.11"));
    assert!(declares_version(
        "本手册针对 `qubit-command` 0.11 版本，面向开发者。",
        "0.11"
    ));

    for declaration in [
        "0.11.1",
        "0.110",
        "0.11beta",
        "0.11,unexpected",
        "0.11，unexpected",
        "0.11。unexpected",
    ] {
        assert!(!declares_version(
            &format!("This guide describes `qubit-command` {declaration}."),
            "0.11"
        ));
    }
    assert!(!declares_version("This guide describes qubit-command 0.11.", "0.11"));
}

#[test]
fn test_documentation_install_constraints_match_the_crate_compatibility_line() {
    let version = CARGO_TOML
        .lines()
        .find_map(|line| line.strip_prefix("version = \""))
        .and_then(|line| line.strip_suffix('\"'))
        .expect("Cargo.toml should declare a package version");
    let compatibility_line = compatibility_line(version);
    let dependency = format!("qubit-command = \"{compatibility_line}\"");

    for (name, contents) in [
        ("README.md", README_EN),
        ("README.zh_CN.md", README_ZH),
        ("doc/user_guide.md", GUIDE_EN),
        ("doc/user_guide.zh_CN.md", GUIDE_ZH),
    ] {
        assert!(contents.contains(&dependency), "{name} should use {dependency}");
        assert!(
            declares_version(contents, &compatibility_line),
            "{name} should declare {compatibility_line}"
        );
    }
}
