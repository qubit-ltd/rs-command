use std::fs::OpenOptions;
use std::io::{Read, Seek, SeekFrom, Write};

use qubit_command::{CommandResourceRunOptions, FileSinkPolicy};

#[test]
fn owned_file_options_keep_current_cursor_and_explicit_policy() {
    let mut input = tempfile::tempfile().expect("input file should be created");
    input
        .write_all(b"prefix-payload")
        .expect("fixture should be written");
    input
        .seek(SeekFrom::Start(7))
        .expect("cursor should be positioned");

    let output = tempfile::NamedTempFile::new().expect("output file should be created");
    std::fs::write(output.path(), b"existing contents").expect("output should be populated");
    let output = OpenOptions::new()
        .read(true)
        .write(true)
        .open(output.path())
        .expect("output should be opened");

    let options = CommandResourceRunOptions::new()
        .stdin_file(input)
        .stdout_sink(output, FileSinkPolicy::Preserve);
    assert!(options.configured_stdin().is_some());
    let (_, policy) = options
        .configured_stdout()
        .expect("stdout sink should be retained");
    assert_eq!(policy, FileSinkPolicy::Preserve);
    assert!(options.configured_stderr().is_none());

    let mut configured_input = options
        .configured_stdin()
        .expect("stdin should remain owned");
    let mut payload = Vec::new();
    configured_input
        .read_to_end(&mut payload)
        .expect("stdin should be readable from current cursor");
    assert_eq!(payload, b"payload");
}

#[test]
fn sink_policies_are_explicit_and_distinct() {
    assert_ne!(FileSinkPolicy::Preserve, FileSinkPolicy::Truncate);
}
