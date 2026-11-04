// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
mod support;

use support::documentation_examples::Example;
use support::documentation_examples::check_documents;
use support::documentation_examples::extract_examples;
use support::documentation_examples::require_markers;

#[test]
fn extractor_preserves_hidden_lines_and_rejects_invalid_markers() {
    let parsed = extract_examples(
        "<!-- doc-example:sample:compile -->\n```rust\nlet x = 1;\n# assert_eq!(x, 1);\n## shown\n```\n",
        "sample.md",
    )
    .expect("valid marked example should extract");
    require_markers(&parsed, &[("sample", "compile")], "sample.md")
        .expect("the example should have its expected mode");
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
    let wrong_mode = extract_examples(
        "<!-- doc-example:sample:unix -->\n```rust\n1;\n```",
        "sample.md",
    )
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
        extract_examples(
            "<!-- doc-example:sample:run-->\n```rust\n1;\n```",
            "sample.md"
        )
        .unwrap_err()
        .contains("malformed marker syntax")
    );
    assert!(
        extract_examples(
            "<!-- doc-example:a:run -->\ntext\n```rust\n1;\n```",
            "sample.md",
        )
        .unwrap_err()
        .contains("immediately followed")
    );
    assert!(
        extract_examples("<!-- doc-example:a:run -->\n```rust\n1;", "sample.md")
            .unwrap_err()
            .contains("unclosed")
    );
    assert!(
        extract_examples(
            "<!-- doc-example:a:run -->\n```rust,ignore\n1;\n```",
            "sample.md"
        )
        .is_err()
    );
}

#[test]
fn extractor_tracks_fragments_and_validates_fence_attributes() {
    let parsed = extract_examples(
        "<!-- doc-example:context:fragment -->\n```rust\nlet value = 1;\n```\n<!-- doc-example:complete:compile -->\n```rust\nassert_eq!(1, 1);\n```\n```text\nignored\n```\n",
        "sample.md",
    )
    .expect("fragment and compile examples should be classified");
    assert_eq!(parsed.len(), 2);
    assert_eq!(parsed[0].mode, "fragment");
    assert_eq!(parsed[1].id, "complete");

    assert!(
        extract_examples(
            "<!-- doc-example:x:fragment -->\n```rust,no_run\n1;\n```",
            "sample.md"
        )
        .is_err()
    );
    assert!(
        extract_examples(
            "<!-- doc-example:x:no_run -->\n```rust\n1;\n```",
            "sample.md"
        )
        .is_err()
    );
    assert!(
        extract_examples(
            "<!-- doc-example:x:fragment -->\n```rust,no_run\n1;\n```",
            "sample.md"
        )
        .is_err()
    );
}

#[test]
fn extractor_recognizes_commonmark_rust_fences_and_skips_nested_looking_text() {
    for source in ["  ```rust\n1;\n  ```", "~~~rust\n1;\n~~~"] {
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
fn bilingual_guide_examples_have_matching_ids_and_modes() {
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let english = std::fs::read_to_string(root.join("doc/user_guide.md")).unwrap();
    let chinese = std::fs::read_to_string(root.join("doc/user_guide.zh_CN.md")).unwrap();
    let en = extract_examples(&english, "doc/user_guide.md").unwrap();
    let zh = extract_examples(&chinese, "doc/user_guide.zh_CN.md").unwrap();
    let mut en_pairs: Vec<_> = en.iter().map(|e| (&e.id, &e.mode)).collect();
    let mut zh_pairs: Vec<_> = zh.iter().map(|e| (&e.id, &e.mode)).collect();
    en_pairs.sort();
    zh_pairs.sort();
    assert_eq!(
        en_pairs, zh_pairs,
        "bilingual guide example IDs/modes differ"
    );
}

#[test]
fn actual_markdown_examples_compile_and_run_for_this_platform() {
    let root = std::env::var_os("DOCUMENTATION_EXAMPLES_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    check_documents(&root);
}
