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
    require_markers(&parsed, &[("sample", "compile")], "sample.md").expect("the example should have its expected mode");
    assert_eq!(
        parsed,
        [Example {
            id: "sample".into(),
            mode: "compile".into(),
            code: "let x = 1;\nassert_eq!(x, 1);\n# shown\n".into()
        }]
    );

    let absent = extract_examples("```rust\n1;\n```", "sample.md").unwrap();
    assert!(absent.is_empty());
    assert!(
        require_markers(&absent, &[("wanted", "compile")], "sample.md")
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
fn actual_markdown_examples_compile_and_run_for_this_platform() {
    let root = std::env::var_os("DOCUMENTATION_EXAMPLES_ROOT")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    check_documents(&root);
}
