// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
mod support;

use serde_json::json;
use support::documentation_dependency_baseline::compare_metadata;
use support::documentation_dependency_baseline::harness_metadata_command;
use support::documentation_dependency_baseline::harness_test_command;
use support::documentation_dependency_baseline::read_lock_seed;
use support::documentation_dependency_baseline::resolve_target_dir;
use support::documentation_dependency_baseline::root_fetch_command;
use support::documentation_dependency_baseline::root_metadata_command;
use support::documentation_examples::Example;
use support::documentation_examples::check_documents;
use support::documentation_examples::extract_examples;
use support::documentation_examples::require_markers;

fn metadata(
    a_version: &str,
    b_version: &str,
    b_source: &str,
    edge_target: &str,
    dev_only: bool,
    extra_baseline_feature: bool,
) -> serde_json::Value {
    let mut packages = vec![
        json!({"id":"qubit-command 0.1.0 (path+file:///tmp/project)","name":"qubit-command","version":"0.1.0","source":null,"manifest_path":"/tmp/project/Cargo.toml"}),
        json!({"id":"a 1.0.0 (registry+https://example.test/index)","name":"a","version":a_version,"source":"registry+https://example.test/index","manifest_path":"/cache/a/Cargo.toml"}),
        json!({"id":format!("b {b_version} ({b_source})"),"name":"b","version":b_version,"source":b_source,"manifest_path":"/cache/b/Cargo.toml"}),
    ];
    if dev_only {
        packages.push(json!({"id":"dev-only 1.0.0 (registry+https://example.test/index)","name":"dev-only","version":"1.0.0","source":"registry+https://example.test/index","manifest_path":"/cache/dev/Cargo.toml"}));
    }
    if extra_baseline_feature {
        packages.push(json!({"id":"feature-extra 1.0.0 (registry+https://example.test/index)","name":"feature-extra","version":"1.0.0","source":"registry+https://example.test/index","manifest_path":"/cache/extra/Cargo.toml"}));
    }
    let b_id = format!("b {b_version} ({b_source})");
    let mut nodes = vec![
        json!({"id":"qubit-command 0.1.0 (path+file:///tmp/project)","deps":[{"name":"a","pkg":"a 1.0.0 (registry+https://example.test/index)","dep_kinds":[{"kind":null,"target":null}]}]}),
        json!({"id":"a 1.0.0 (registry+https://example.test/index)","deps":[{"name":"b","pkg":edge_target,"dep_kinds":[{"kind":null,"target":null}]}]}),
        json!({"id":b_id,"deps":[]}),
    ];
    if dev_only {
        nodes[0]["deps"].as_array_mut().unwrap().push(json!({"name":"dev-only","pkg":"dev-only 1.0.0 (registry+https://example.test/index)","dep_kinds":[{"kind":"dev","target":null}]}));
    }
    if extra_baseline_feature {
        nodes.push(json!({"id":"feature-extra 1.0.0 (registry+https://example.test/index)","deps":[]}));
        nodes[0]["deps"].as_array_mut().unwrap().push(json!({"name":"feature-extra","pkg":"feature-extra 1.0.0 (registry+https://example.test/index)","dep_kinds":[{"kind":null,"target":null}]}));
    }
    json!({"packages":packages,"resolve":{"nodes":nodes}})
}

#[test]
fn test_relative_target_directory_is_resolved_from_package_root() {
    let root = std::path::Path::new("/workspace/project");
    assert_eq!(
        resolve_target_dir(root, Some(std::ffi::OsStr::new("build/cache"))),
        root.join("build/cache")
    );
    assert_eq!(
        resolve_target_dir(root, Some(std::ffi::OsStr::new("/tmp/cargo-target"))),
        std::path::PathBuf::from("/tmp/cargo-target")
    );
    assert_eq!(resolve_target_dir(root, None), root.join("target"));
}

fn baseline_fixture(b_version: &str, b_source: &str) -> serde_json::Value {
    metadata(
        "1.0.0",
        b_version,
        b_source,
        &format!("b {b_version} ({b_source})"),
        false,
        false,
    )
}

#[test]
fn test_production_dependency_baseline_compares_packages_sources_and_edges() {
    let base = std::path::Path::new("/tmp/project");
    let v1_source = "registry+https://example.test/index";
    let baseline = baseline_fixture("1.0.0", v1_source);
    let matching = metadata(
        "1.0.0",
        "1.0.0",
        v1_source,
        &format!("b 1.0.0 ({v1_source})"),
        false,
        false,
    );
    assert!(compare_metadata(&baseline, &matching, base).is_ok());

    let newer = metadata(
        "1.0.0",
        "2.0.0",
        v1_source,
        &format!("b 2.0.0 ({v1_source})"),
        false,
        false,
    );
    assert!(
        compare_metadata(&baseline, &newer, base)
            .unwrap_err()
            .contains("b@2.0.0")
    );
    let other_source = "git+https://example.test/repo#abc";
    let source_changed = metadata(
        "1.0.0",
        "1.0.0",
        other_source,
        &format!("b 1.0.0 ({other_source})"),
        false,
        false,
    );
    assert!(
        compare_metadata(&baseline, &source_changed, base)
            .unwrap_err()
            .contains("b@1.0.0")
    );

    let b2_id = format!("b 2.0.0 ({v1_source})");
    let mut two_version_baseline = baseline.clone();
    two_version_baseline["packages"].as_array_mut().unwrap().push(
        json!({"id":&b2_id,"name":"b","version":"2.0.0","source":v1_source,"manifest_path":"/cache/b2/Cargo.toml"}),
    );
    two_version_baseline["resolve"]["nodes"]
        .as_array_mut()
        .unwrap()
        .push(json!({"id":&b2_id,"deps":[]}));
    two_version_baseline["resolve"]["nodes"][0]["deps"]
        .as_array_mut()
        .unwrap()
        .push(json!({"name":"b-alias","pkg":&b2_id,"dep_kinds":[{"kind":null,"target":null}]}));
    let mut changed_edge = two_version_baseline.clone();
    changed_edge["resolve"]["nodes"][1]["deps"][0]["pkg"] = json!(b2_id);
    assert!(
        compare_metadata(&two_version_baseline, &changed_edge, base)
            .unwrap_err()
            .contains("--b (normal:)-->")
    );
    let dev_extra = metadata(
        "1.0.0",
        "1.0.0",
        v1_source,
        &format!("b 1.0.0 ({v1_source})"),
        true,
        false,
    );
    assert!(compare_metadata(&baseline, &dev_extra, base).is_ok());
    let feature_baseline = metadata(
        "1.0.0",
        "1.0.0",
        v1_source,
        &format!("b 1.0.0 ({v1_source})"),
        false,
        true,
    );
    assert!(compare_metadata(&feature_baseline, &matching, base).is_ok());
}

#[test]
fn test_cargo_commands_use_fixed_root_manifest_and_required_lock_offline_flags() {
    use std::ffi::OsStr;
    use std::path::Path;

    fn args(command: &std::process::Command) -> Vec<String> {
        command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }
    let root = Path::new("/workspace/project");
    let harness_manifest = Path::new("/tmp/harness/Cargo.toml");
    let target = Path::new("/tmp/target");
    let cargo = OsStr::new("cargo");
    let fetch_command = root_fetch_command(cargo, root);
    let root_manifest = root.join("Cargo.toml").to_string_lossy().into_owned();
    assert_eq!(fetch_command.get_current_dir(), Some(root));
    assert_eq!(
        args(&fetch_command),
        vec![
            "fetch".to_owned(),
            "--locked".to_owned(),
            "--manifest-path".to_owned(),
            root_manifest.clone(),
        ]
    );
    let root_command = root_metadata_command(cargo, root);
    assert_eq!(root_command.get_current_dir(), Some(root));
    assert_eq!(
        args(&root_command),
        vec![
            "metadata".to_owned(),
            "--format-version".to_owned(),
            "1".to_owned(),
            "--locked".to_owned(),
            "--offline".to_owned(),
            "--manifest-path".to_owned(),
            root_manifest,
        ]
    );
    let metadata_command = harness_metadata_command(cargo, root, harness_manifest);
    let harness_manifest_arg = harness_manifest.to_string_lossy().into_owned();
    assert_eq!(metadata_command.get_current_dir(), Some(root));
    assert_eq!(
        args(&metadata_command),
        vec![
            "metadata".to_owned(),
            "--format-version".to_owned(),
            "1".to_owned(),
            "--offline".to_owned(),
            "--manifest-path".to_owned(),
            harness_manifest_arg.clone(),
        ]
    );
    let test_command = harness_test_command(cargo, root, harness_manifest, target);
    let target_arg = target.to_string_lossy().into_owned();
    assert_eq!(test_command.get_current_dir(), Some(root));
    assert_eq!(
        args(&test_command),
        vec![
            "test".to_owned(),
            "--manifest-path".to_owned(),
            harness_manifest_arg,
            "--target-dir".to_owned(),
            target_arg,
            "--locked".to_owned(),
            "--offline".to_owned(),
        ]
    );
}

#[test]
fn test_missing_root_lock_seed_is_reported_as_not_found() {
    let temp = tempfile::tempdir().unwrap();
    let missing = temp.path().join("Cargo.lock");
    let error = read_lock_seed(&missing).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
}

#[test]
fn test_extractor_preserves_hidden_lines_and_rejects_invalid_markers() {
    let parsed = extract_examples(
        "<!-- doc-example:sample:compile -->\n```rust\nlet x = 1;\n# assert_eq!(x, 1);\n## shown\n```\n",
        "sample.md",
    )
    .expect("valid marked example should extract");
    require_markers(&parsed, &[("sample", "compile")], "sample.md").expect("the example should have its expected mode");
    assert_eq!(
        parsed,
        [Example {
            id: "sample".into(),
            mode: "compile".into(),
            code: "let x = 1;\nassert_eq!(x, 1);\n# shown\n".into()
        }]
    );

    let absent = extract_examples("```rust\n1;\n```", "sample.md").unwrap_err();
    assert!(absent.contains("sample.md:1"));
    assert!(absent.contains("unclassified Rust fence"));
    assert!(
        require_markers(&[], &[("wanted", "compile")], "sample.md")
            .unwrap_err()
            .contains("missing required marker")
    );
    let wrong_mode = extract_examples("<!-- doc-example:sample:unix -->\n```rust\n1;\n```", "sample.md")
        .expect("valid mode syntax should extract");
    assert!(
        require_markers(&wrong_mode, &[("sample", "windows")], "sample.md")
            .unwrap_err()
            .contains("must use mode windows")
    );
    assert!(
        extract_examples(
            "<!-- doc-example:a:run -->\n```rust\n1;\n```\n<!-- doc-example:a:run -->\n```rust\n2;\n```",
            "sample.md",
        )
        .unwrap_err()
        .contains("duplicate")
    );
    assert!(
        extract_examples("<!-- doc-example:a:run -->", "sample.md")
            .unwrap_err()
            .contains("no following")
    );
    assert!(
        extract_examples("<!-- doc-example:broken -->\n```rust\n1;\n```", "sample.md")
            .unwrap_err()
            .contains("malformed marker")
    );
    assert!(
        extract_examples("<!-- doc-example:sample:run-->\n```rust\n1;\n```", "sample.md")
            .unwrap_err()
            .contains("malformed marker syntax")
    );
    assert!(
        extract_examples("<!-- doc-example:a:run -->\ntext\n```rust\n1;\n```", "sample.md",)
            .unwrap_err()
            .contains("immediately followed")
    );
    assert!(
        extract_examples("<!-- doc-example:a:run -->\n```rust\n1;", "sample.md")
            .unwrap_err()
            .contains("unclosed")
    );
    assert!(extract_examples("<!-- doc-example:a:run -->\n```rust,ignore\n1;\n```", "sample.md").is_err());
}

#[test]
fn test_extractor_tracks_fragments_and_validates_fence_attributes() {
    let parsed = extract_examples(
        "<!-- doc-example:context:fragment -->\n```rust\nlet value = 1;\n```\n<!-- doc-example:complete:compile -->\n```rust\nassert_eq!(1, 1);\n```\n```text\nignored\n```\n",
        "sample.md",
    )
    .expect("fragment and compile examples should be classified");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].mode, "fragment");
    assert_eq!(parsed[1].id, "complete");

    assert!(extract_examples("<!-- doc-example:x:fragment -->\n```rust,no_run\n1;\n```", "sample.md").is_err());
    assert!(extract_examples("<!-- doc-example:x:no_run -->\n```rust\n1;\n```", "sample.md").is_err());
    assert!(extract_examples("<!-- doc-example:x:fragment -->\n```rust,no_run\n1;\n```", "sample.md").is_err());
}

#[test]
fn test_extractor_recognizes_commonmark_rust_fences_and_skips_nested_looking_text() {
    for source in ["  ```rust\n1;\n  ```", "~~~rust\n1;\n~~~", "```rust extra\n1;\n```"] {
        let error = extract_examples(source, "sample.md").unwrap_err();
        assert!(error.contains("unclassified Rust fence"), "{error}");
        assert!(error.contains("sample.md:1"), "{error}");
    }

    let parsed = extract_examples(
        "<!-- doc-example:indented:compile -->\n  ```rust\nlet a = 1;\n  ```\n<!-- doc-example:tilde:compile -->\n~~~~rust\nlet b = 2;\n~~~~\n~~~text\n```rust\nnot a real outer close\n```\n~~~\n",
        "sample.md",
    )
    .expect("indented and tilde fences should parse, while non-Rust content is skipped");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].code, "let a = 1;\n");
    assert_eq!(parsed[1].code, "let b = 2;\n");
}

#[test]
fn test_bilingual_guide_examples_have_matching_ids_and_modes() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let english = std::fs::read_to_string(root.join("doc/user_guide.md")).unwrap();
    let chinese = std::fs::read_to_string(root.join("doc/user_guide.zh_CN.md")).unwrap();
    let en = extract_examples(&english, "doc/user_guide.md").unwrap();
    let zh = extract_examples(&chinese, "doc/user_guide.zh_CN.md").unwrap();
    let mut en_pairs: Vec<_> = en.iter().map(|e| (&e.id, &e.mode)).collect();
    let mut zh_pairs: Vec<_> = zh.iter().map(|e| (&e.id, &e.mode)).collect();
    en_pairs.sort();
    zh_pairs.sort();
    assert_eq!(en_pairs, zh_pairs, "bilingual guide example IDs/modes differ");
}

#[test]
fn test_actual_markdown_examples_compile_and_run_for_this_platform() {
    let root = std::env::var_os("DOCUMENTATION_EXAMPLES_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    check_documents(&root);
}
