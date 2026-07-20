// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests for command arguments carrying explicit diagnostic sensitivity.

use std::ffi::OsStr;
#[cfg(unix)]
use std::{
    ffi::OsString,
    os::unix::ffi::OsStringExt,
};

use qubit_command::Command;

#[test]
fn test_command_debug_redacts_sensitive_positional_argument() {
    let command = Command::new("ffprobe")
        .arg("-i")
        .sensitive_arg("/private/customer/video.mp4");

    let debug = format!("{command:?}");

    assert!(debug.contains("<redacted>"));
    assert!(!debug.contains("/private/customer/video.mp4"));
}

#[test]
fn test_command_sensitive_os_arg_preserves_raw_value_and_redacts_debug() {
    let path = OsStr::new("/private/customer/video.mp4");
    let command = Command::new("ffprobe").sensitive_arg_os(path);

    assert_eq!(
        command
            .arguments()
            .next()
            .expect("sensitive argument should be retained"),
        path,
    );
    assert!(!format!("{command:?}").contains("/private/customer/video.mp4"));
}

#[cfg(unix)]
#[test]
fn test_command_sensitive_non_utf8_argument_never_leaks_lossy_fragments() {
    let value = OsString::from_vec(b"prefix-secret-\xFF-suffix".to_vec());
    let command = Command::new("tool").sensitive_arg_os(&value);

    let debug = format!("{command:?}");

    assert!(!debug.contains("prefix-secret"));
    assert!(!debug.contains("suffix"));
    assert_eq!(
        command
            .arguments()
            .next()
            .expect("the sensitive argument should be retained"),
        value,
    );
}
