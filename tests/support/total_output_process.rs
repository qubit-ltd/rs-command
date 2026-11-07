// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Independent watchdog and self-executing fixtures for total-output tests.

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command as ProcessCommand;
use std::process::Stdio;
use std::thread;
use std::time::Duration;
use std::time::Instant;

use qubit_command::Command;
use qubit_command::CommandErrorKind;
use qubit_command::CommandErrorReason;
use qubit_command::CommandRunner;

const ROLE_ENV: &str = "QUBIT_TOTAL_OUTPUT_TEST_ROLE";
const CONTROL_ENV: &str = "QUBIT_TOTAL_OUTPUT_TEST_CONTROL";
const STREAM_ENV: &str = "QUBIT_TOTAL_OUTPUT_TEST_STREAM";
const FIXTURE_ENV: &str = "QUBIT_TOTAL_OUTPUT_TEST_FIXTURE";
const PROBE_MODE_ENV: &str = "QUBIT_TOTAL_OUTPUT_TEST_PROBE_MODE";
const START_FILE: &str = "start";
const EMIT_FILE: &str = "emit";
const ABORT_FILE: &str = "abort";
const DIRECT_PID_FILE: &str = "direct.pid";
const DESCENDANT_PID_FILE: &str = "descendant.pid";
const READY_FILE: &str = "ready";
const SUCCESS_FILE: &str = "success";
const PROBE: &[u8; 9] = b"123456789";

#[derive(Clone, Copy)]
pub(crate) enum Stream {
    Stdout,
    Stderr,
}

impl Stream {
    fn name(self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

/// Dispatches a self-executed fixture or supervises one real budget case.
pub(crate) fn run(test_name: &str, stream: Stream) {
    match std::env::var(ROLE_ENV).as_deref() {
        Ok("worker") => worker(test_name, stream),
        _ => supervise(test_name, stream),
    }
}

/// Runs bounded watchdog, readiness, failure, and residual-child self-tests.
pub(crate) fn test_supervisor_failures(test_name: &str) {
    if std::env::var(ROLE_ENV).as_deref() == Ok("probe") {
        supervisor_probe();
        return;
    }
    let control = tempfile::Builder::new()
        .prefix("qubit-total-output-probe-")
        .tempdir()
        .expect("probe control directory should be created");
    let fixture = compile_fixture(control.path());
    for mode in ["not-ready", "hang", "failure", "residual"] {
        exercise_probe(test_name, control.path(), &fixture, mode);
    }
}

fn supervisor_probe() {
    let control = control_dir();
    wait_for_file(&control.join(START_FILE), Duration::from_secs(5)).expect("supervisor should release probe worker");
    let mode = std::env::var(PROBE_MODE_ENV).expect("probe mode should be set");
    if mode != "not-ready" {
        atomic_write(&control.join(READY_FILE), b"ready").expect("probe should publish readiness");
    }
    match mode.as_str() {
        "not-ready" | "hang" => loop {
            thread::sleep(Duration::from_secs(1));
        },
        "failure" => panic!("injected supervisor worker failure"),
        "residual" => {
            let fixture = std::env::var(FIXTURE_ENV).expect("probe fixture should be set");
            let mut command = ProcessCommand::new(fixture);
            command
                .arg("command")
                .env(CONTROL_ENV, &control)
                .env(STREAM_ENV, "stdout")
                .stdout(Stdio::null())
                .stderr(Stdio::null());
            #[cfg(unix)]
            {
                use std::os::unix::process::CommandExt;
                command.process_group(0);
            }
            let child = command.spawn().expect("residual child should start");
            wait_for_file(&control.join("descendant.ready"), Duration::from_secs(5))
                .expect("residual fixture direct and descendant processes should become ready");
            // This fixture deliberately leaves a child for the outer boundary to detect.
            std::mem::forget(child);
        }
        _ => panic!("unknown supervisor probe mode"),
    }
}

fn exercise_probe(test_name: &str, control: &Path, fixture: &Path, mode: &str) {
    let _ = fs::remove_file(control.join(READY_FILE));
    let _ = fs::remove_file(control.join(DESCENDANT_PID_FILE));
    let mut command =
        ProcessCommand::new(std::env::current_exe().expect("integration test executable should be available"));
    command
        .args(["--exact", test_name, "--quiet", "--nocapture", "--test-threads=1"])
        .env(ROLE_ENV, "probe")
        .env(PROBE_MODE_ENV, mode)
        .env(CONTROL_ENV, control)
        .env(FIXTURE_ENV, fixture)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    configure_supervisor(&mut command);
    let mut child = command.spawn().expect("supervisor probe worker should start");
    let mut boundary = SupervisorBoundary::assign(&child);
    atomic_write(&control.join(START_FILE), b"start").expect("probe worker should be released");
    match mode {
        "not-ready" => {
            assert!(wait_for_file(&control.join(READY_FILE), Duration::from_millis(200)).is_err());
            boundary.terminate();
            let status = child.wait().expect("watchdog should reap unready worker");
            assert!(!status.success(), "unready worker must not pass its self-test");
            wait_for_boundary_empty(&boundary, Duration::from_secs(5))
                .expect("unready worker boundary should be empty after cleanup");
        }
        "hang" => {
            wait_for_file(&control.join(READY_FILE), Duration::from_secs(5))
                .expect("hanging worker should reach readiness");
            assert!(child.try_wait().expect("probe exit should be observable").is_none());
            boundary.terminate();
            let status = child.wait().expect("watchdog should reap hanging worker");
            assert!(!status.success(), "watchdog-terminated worker must fail");
            wait_for_boundary_empty(&boundary, Duration::from_secs(5))
                .expect("hanging worker boundary should be empty after cleanup");
        }
        "failure" => {
            wait_for_file(&control.join(READY_FILE), Duration::from_secs(5))
                .expect("failing worker should publish readiness first");
            let status = child.wait().expect("failed worker should be reaped");
            assert!(!status.success(), "injected worker failure must be observed");
            assert!(boundary.is_empty().expect("failed worker should leave no child"));
        }
        "residual" => {
            wait_for_file(&control.join(READY_FILE), Duration::from_secs(5))
                .expect("residual worker should publish readiness");
            let status = child.wait().expect("residual worker should return normally");
            assert!(status.success(), "residual worker should return normally");
            let direct_identity = identify_active(read_pid(&control.join(DIRECT_PID_FILE)))
                .expect("residual direct process should remain active after worker exit");
            let identity = identify_active(read_pid(&control.join(DESCENDANT_PID_FILE)))
                .expect("residual descendant must remain active after worker exit");
            #[cfg(unix)]
            assert!(
                boundary.is_empty().expect("worker process group should be observable"),
                "a process in its own group should escape the worker-group boundary"
            );
            #[cfg(windows)]
            assert!(
                !boundary.is_empty().expect("worker Job Object should be observable"),
                "a child should remain within the worker Job Object"
            );
            #[cfg(unix)]
            terminate_fixture_group(control);
            #[cfg(windows)]
            terminate_fixture_tree(control);
            boundary.terminate();
            wait_until_stopped(&direct_identity, Duration::from_secs(5))
                .expect("supervisor should terminate residual direct process group");
            wait_until_stopped(&identity, Duration::from_secs(5))
                .expect("supervisor should stop and reap residual descendant");
            wait_for_boundary_empty(&boundary, Duration::from_secs(5))
                .expect("residual child tree should be terminated");
        }
        _ => unreachable!(),
    }
}

fn supervise(test_name: &str, stream: Stream) {
    let control = tempfile::Builder::new()
        .prefix("qubit-total-output-")
        .tempdir()
        .expect("supervisor control directory should be created");
    let fixture = compile_fixture(control.path());
    let mut worker =
        ProcessCommand::new(std::env::current_exe().expect("integration test executable should be available"));
    worker
        .args(["--exact", test_name, "--quiet", "--nocapture", "--test-threads=1"])
        .env(ROLE_ENV, "worker")
        .env(CONTROL_ENV, control.path())
        .env(STREAM_ENV, stream.name())
        .env(FIXTURE_ENV, fixture)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    configure_supervisor(&mut worker);
    let mut worker = worker.spawn().expect("supervised worker should start");
    let mut boundary = SupervisorBoundary::assign(&worker);
    atomic_write(&control.path().join(START_FILE), b"start").expect("supervisor should release the worker");

    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        match worker.try_wait().expect("worker exit should be observable") {
            Some(status) => {
                if !boundary
                    .is_empty()
                    .expect("supervisor process boundary should be observable")
                {
                    terminate_fixture_tree(control.path());
                    boundary.terminate();
                    wait_for_boundary_empty(&boundary, Duration::from_secs(5))
                        .expect("supervisor should terminate leftover worker process tree");
                    panic!("worker exited with a live process in its independent supervisor boundary");
                }
                assert!(status.success(), "supervised worker failed: {status}");
                assert!(
                    control.path().join(SUCCESS_FILE).exists(),
                    "worker must publish success only after process-tree assertions"
                );
                return;
            }
            None if Instant::now() < deadline => thread::sleep(Duration::from_millis(10)),
            None => {
                terminate_fixture_tree(control.path());
                boundary.terminate();
                worker.wait().expect("watchdog should reap its worker");
                wait_for_boundary_empty(&boundary, Duration::from_secs(5))
                    .expect("watchdog should terminate the complete worker process tree");
                panic!("total-output worker exceeded the independent 30 second watchdog");
            }
        }
    }
}

/// Cleans up the runner's process group, which is separate from the worker's
/// group on Unix when process-tree management is enabled.
fn terminate_fixture_tree(control: &Path) {
    let direct_pid = read_optional_pid(&control.join(DIRECT_PID_FILE));
    let descendant_pid = read_optional_pid(&control.join(DESCENDANT_PID_FILE));
    #[cfg(unix)]
    if let Some(pid) = direct_pid
        && identify_active(pid).is_ok()
    {
        terminate_process_group(pid);
    }
    #[cfg(windows)]
    for pid in [descendant_pid, direct_pid].into_iter().flatten() {
        terminate_process(pid);
    }
    #[cfg(unix)]
    if let Some(pid) = descendant_pid
        && identify_active(pid).is_ok()
    {
        let result = unsafe { libc::kill(pid as i32, libc::SIGKILL) };
        assert!(result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH));
    }
}

#[cfg(unix)]
fn terminate_fixture_group(control: &Path) {
    if let Some(pid) = read_optional_pid(&control.join(DIRECT_PID_FILE))
        && identify_active(pid).is_ok()
    {
        terminate_process_group(pid);
    }
}

#[cfg(unix)]
fn terminate_process_group(pid: u32) {
    let result = unsafe { libc::kill(-(pid as i32), libc::SIGKILL) };
    assert!(result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH));
}

fn read_optional_pid(path: &Path) -> Option<u32> {
    fs::read_to_string(path).ok()?.trim().parse().ok()
}

#[cfg(windows)]
fn terminate_process(pid: u32) {
    use std::os::windows::io::FromRawHandle;
    use std::os::windows::io::OwnedHandle;
    use std::os::windows::io::RawHandle;

    const PROCESS_TERMINATE: u32 = 0x0001;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> RawHandle;
        fn TerminateProcess(process: RawHandle, exit_code: u32) -> i32;
        fn GetLastError() -> u32;
    }
    let raw = unsafe { OpenProcess(PROCESS_TERMINATE, 0, pid) };
    if raw.is_null() {
        return;
    }
    let _handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    let terminated = unsafe { TerminateProcess(raw, 1) };
    assert!(
        terminated != 0,
        "terminate fixture process {pid}: Windows error {}",
        unsafe { GetLastError() }
    );
}

fn worker(_test_name: &str, stream: Stream) {
    #[cfg(target_os = "linux")]
    set_child_subreaper();
    let control = control_dir();
    wait_for_file(&control.join(START_FILE), Duration::from_secs(5))
        .expect("supervisor should release worker before its readiness deadline");

    let control_string = control.to_str().expect("control directory path should be valid UTF-8");
    let fixture = std::env::var(FIXTURE_ENV).expect("supervisor should provide compiled fixture");
    let command = Command::new(&fixture)
        .args(&["command"])
        .env(CONTROL_ENV, control_string)
        .env(STREAM_ENV, stream.name());
    let (result_tx, result_rx) = std::sync::mpsc::channel();
    thread::spawn(move || {
        let result = CommandRunner::without_timeout().max_total_output_bytes(8).run(command);
        let _ = result_tx.send(result);
    });
    if let Err(error) = wait_for_file(&control.join(READY_FILE), Duration::from_secs(5)) {
        atomic_write(&control.join(ABORT_FILE), b"abort").expect("abort gate should open on setup failure");
        let _ = result_rx.recv_timeout(Duration::from_secs(5));
        panic!("process fixture did not become ready: {error}");
    }
    let direct_pid = read_pid(&control.join(DIRECT_PID_FILE));
    let descendant_pid = read_pid(&control.join(DESCENDANT_PID_FILE));
    let direct_identity =
        identify_active(direct_pid).expect("direct child identity must be active before output is released");
    let descendant_identity =
        identify_active(descendant_pid).expect("ready descendant identity must be active before output is released");
    atomic_write(&control.join(EMIT_FILE), b"emit").expect("worker should release output gate");
    let runner_result = result_rx
        .recv_timeout(Duration::from_secs(5))
        .expect("runner should return after its independent output budget is exceeded");
    let runner_error = runner_result.expect_err("a live process tree must exceed the output budget");
    assert_eq!(runner_error.kind(), CommandErrorKind::OutputLimitExceeded);
    assert!(matches!(
        runner_error.reason(),
        CommandErrorReason::OutputLimitExceeded { limit: 8, observed: 9 }
    ));
    assert!(runner_error.output().is_none());
    let captured = runner_error
        .captured_output()
        .expect("budget errors retain captured output");
    match stream {
        Stream::Stdout => {
            assert_eq!(captured.stdout(), PROBE);
            assert!(!captured.stdout_complete());
            assert!(captured.stderr().is_empty());
        }
        Stream::Stderr => {
            assert_eq!(captured.stderr(), PROBE);
            assert!(!captured.stderr_complete());
            assert!(captured.stdout().is_empty());
        }
    }
    assert_eq!(captured.stdout().len() + captured.stderr().len(), 9);

    wait_until_stopped(&direct_identity, Duration::from_secs(5))
        .expect("runner direct child must stop before fallback cleanup");
    wait_until_stopped(&descendant_identity, Duration::from_secs(5))
        .expect("ready descendant must stop before fallback cleanup");
    atomic_write(&control.join(SUCCESS_FILE), b"success")
        .expect("worker should publish successful process-tree observation");
}

fn control_dir() -> PathBuf {
    PathBuf::from(std::env::var_os(CONTROL_ENV).expect("fixture control path should be set"))
}

fn compile_fixture(directory: &Path) -> PathBuf {
    let source_path = directory.join("total-output-fixture.rs");
    let executable_path = directory.join(if cfg!(windows) {
        "total-output-fixture.exe"
    } else {
        "total-output-fixture"
    });
    fs::write(&source_path, FIXTURE_SOURCE).expect("fixture source should be written");
    let output = ProcessCommand::new("rustc")
        .args(["--edition=2024", "-o"])
        .arg(&executable_path)
        .arg(&source_path)
        .output()
        .expect("the project Rust toolchain should compile the process fixture");
    assert!(
        output.status.success(),
        "compile process fixture: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    executable_path
}

const FIXTURE_SOURCE: &str = r#"
use std::fs;
use std::io::Write;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

fn publish(path: &str, value: &[u8]) {
    let temporary = format!("{path}.tmp-{}", std::process::id());
    fs::write(&temporary, value).unwrap();
    fs::rename(temporary, path).unwrap();
}

fn main() {
    let control = std::env::var("QUBIT_TOTAL_OUTPUT_TEST_CONTROL").unwrap();
    let role = std::env::args().nth(1).unwrap();
    if role == "descendant" {
        publish(&format!("{control}/descendant.pid"), std::process::id().to_string().as_bytes());
        publish(&format!("{control}/descendant.ready"), b"ready");
        loop {
            if std::path::Path::new(&format!("{control}/abort")).exists() { return; }
            thread::sleep(Duration::from_millis(10));
        }
    }
    publish(&format!("{control}/direct.pid"), std::process::id().to_string().as_bytes());
    let descendant = Command::new(std::env::current_exe().unwrap())
        .arg("descendant")
        .env("QUBIT_TOTAL_OUTPUT_TEST_CONTROL", &control)
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .spawn()
        .unwrap();
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !std::path::Path::new(&format!("{control}/descendant.ready")).exists() {
        assert!(std::time::Instant::now() < deadline, "descendant readiness timed out");
        thread::sleep(Duration::from_millis(5));
    }
    publish(&format!("{control}/ready"), b"ready");
    while !std::path::Path::new(&format!("{control}/emit")).exists() {
        if std::path::Path::new(&format!("{control}/abort")).exists() { return; }
        thread::sleep(Duration::from_millis(5));
    }
    let bytes = b"123456789";
    match std::env::var("QUBIT_TOTAL_OUTPUT_TEST_STREAM").as_deref() {
        Ok("stdout") => std::io::stdout().write_all(bytes).and_then(|_| std::io::stdout().flush()).unwrap(),
        Ok("stderr") => std::io::stderr().write_all(bytes).and_then(|_| std::io::stderr().flush()).unwrap(),
        _ => panic!("test stream is required"),
    }
    loop { thread::sleep(Duration::from_secs(1)); }
}
"#;

fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let temporary = path.with_extension(format!("tmp-{}", std::process::id()));
    fs::write(&temporary, bytes)?;
    fs::rename(temporary, path)
}

fn read_pid(path: &Path) -> u32 {
    let value = fs::read_to_string(path).expect("fixture should publish process identity");
    value.trim().parse().expect("published PID should be numeric")
}

fn wait_for_file(path: &Path, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if path.exists() {
            return Ok(());
        }
        thread::sleep(Duration::from_millis(5));
    }
    Err(format!("{} was not published within {timeout:?}", path.display()))
}

#[cfg(unix)]
fn configure_supervisor(command: &mut ProcessCommand) {
    use std::os::unix::process::CommandExt;
    command.process_group(0);
}

#[cfg(windows)]
fn configure_supervisor(_command: &mut ProcessCommand) {}

#[cfg(unix)]
struct ProcessIdentity {
    pid: u32,
    start_time: String,
}

#[cfg(unix)]
fn identify_active(pid: u32) -> Result<ProcessIdentity, String> {
    let (state, start_time) = process_snapshot(pid)?;
    if state != ProcessState::Active {
        return Err(format!("process {pid} was not active at readiness"));
    }
    Ok(ProcessIdentity { pid, start_time })
}

#[cfg(unix)]
fn wait_until_stopped(identity: &ProcessIdentity, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        let (state, start_time) = match process_snapshot(identity.pid) {
            Ok(state) => state,
            Err(error) if error == "process identity no longer exists" => return Ok(()),
            Err(error) => return Err(error),
        };
        if start_time != identity.start_time {
            return Ok(());
        }
        if state == ProcessState::Stopped {
            #[cfg(target_os = "linux")]
            reap_adopted_child(identity.pid)?;
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("process {} remained active after {timeout:?}", identity.pid));
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(unix)]
#[derive(PartialEq, Eq)]
enum ProcessState {
    Active,
    Stopped,
}

#[cfg(unix)]
fn process_snapshot(pid: u32) -> Result<(ProcessState, String), String> {
    #[cfg(target_os = "linux")]
    {
        let stat = match fs::read_to_string(format!("/proc/{pid}/stat")) {
            Ok(stat) => stat,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Err("process identity no longer exists".to_owned());
            }
            Err(error) => return Err(format!("read process identity {pid}: {error}")),
        };
        let fields = stat
            .rsplit_once(')')
            .map(|(_, fields)| fields.split_whitespace().collect::<Vec<_>>())
            .ok_or_else(|| format!("parse process state for PID {pid}"))?;
        let state = *fields.first().ok_or_else(|| format!("parse state for PID {pid}"))?;
        let start_time = fields
            .get(19)
            .ok_or_else(|| format!("parse start time for PID {pid}"))?
            .to_string();
        Ok((
            if state == "Z" || state == "X" {
                ProcessState::Stopped
            } else {
                ProcessState::Active
            },
            start_time,
        ))
    }
    #[cfg(not(target_os = "linux"))]
    {
        let output = ProcessCommand::new("ps")
            .args(["-o", "stat=", "-p", &pid.to_string()])
            .output()
            .map_err(|error| format!("query process identity {pid}: {error}"))?;
        if !output.status.success() || output.stdout.is_empty() {
            return Err("process identity no longer exists".to_owned());
        }
        let state = String::from_utf8_lossy(&output.stdout).trim().to_owned();
        let start_output = ProcessCommand::new("ps")
            .args(["-o", "lstart=", "-p", &pid.to_string()])
            .output()
            .map_err(|error| format!("query process start time {pid}: {error}"))?;
        let start_time = String::from_utf8_lossy(&start_output.stdout).trim().to_owned();
        Ok((
            if state.starts_with('Z') {
                ProcessState::Stopped
            } else {
                ProcessState::Active
            },
            start_time,
        ))
    }
}

#[cfg(target_os = "linux")]
fn set_child_subreaper() {
    const PR_SET_CHILD_SUBREAPER: libc::c_int = 36;
    let result = unsafe { libc::prctl(PR_SET_CHILD_SUBREAPER, 1, 0, 0, 0) };
    assert_eq!(result, 0, "worker should adopt orphaned test descendants");
}

#[cfg(target_os = "linux")]
fn reap_adopted_child(pid: u32) -> Result<(), String> {
    let mut status = 0;
    let result = unsafe { libc::waitpid(pid as libc::pid_t, &mut status, libc::WNOHANG) };
    if result == pid as libc::pid_t || result == 0 {
        return Ok(());
    }
    let error = std::io::Error::last_os_error();
    if error.raw_os_error() == Some(libc::ECHILD) {
        Ok(())
    } else {
        Err(format!("reap adopted process {pid}: {error}"))
    }
}

#[cfg(windows)]
struct ProcessIdentity {
    pid: u32,
    handle: std::os::windows::io::OwnedHandle,
}

#[cfg(windows)]
fn identify_active(pid: u32) -> Result<ProcessIdentity, String> {
    use std::os::windows::io::FromRawHandle;
    use std::os::windows::io::OwnedHandle;
    use std::os::windows::io::RawHandle;

    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_TIMEOUT: u32 = 258;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn OpenProcess(access: u32, inherit: i32, process_id: u32) -> RawHandle;
        fn WaitForSingleObject(handle: RawHandle, milliseconds: u32) -> u32;
        fn GetLastError() -> u32;
    }
    let raw = unsafe { OpenProcess(SYNCHRONIZE, 0, pid) };
    if raw.is_null() {
        return Err(format!(
            "open active process identity {pid}: Windows error {}",
            unsafe { GetLastError() }
        ));
    }
    let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
    if unsafe { WaitForSingleObject(raw, 0) } != WAIT_TIMEOUT {
        return Err(format!("process {pid} stopped before output gate release"));
    }
    Ok(ProcessIdentity { pid, handle })
}

#[cfg(windows)]
fn wait_until_stopped(identity: &ProcessIdentity, timeout: Duration) -> Result<(), String> {
    use std::os::windows::io::AsRawHandle;
    use std::os::windows::io::RawHandle;

    const SYNCHRONIZE: u32 = 0x0010_0000;
    const WAIT_OBJECT_0: u32 = 0;
    const WAIT_TIMEOUT: u32 = 258;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn WaitForSingleObject(handle: RawHandle, milliseconds: u32) -> u32;
    }
    let millis = u32::try_from(timeout.as_millis()).unwrap_or(u32::MAX - 1);
    match unsafe { WaitForSingleObject(identity.handle.as_raw_handle(), millis) } {
        WAIT_OBJECT_0 => Ok(()),
        WAIT_TIMEOUT => Err(format!("process {} remained active after {timeout:?}", identity.pid)),
        code => Err(format!("wait for process {} returned {code}", identity.pid)),
    }
}

#[cfg(unix)]
struct SupervisorBoundary(i32);

#[cfg(unix)]
impl SupervisorBoundary {
    fn assign(worker: &std::process::Child) -> Self {
        Self(worker.id() as i32)
    }

    fn is_empty(&self) -> Result<bool, String> {
        #[cfg(target_os = "linux")]
        {
            for entry in fs::read_dir("/proc").map_err(|error| format!("list process table: {error}"))? {
                let entry = entry.map_err(|error| format!("read process table entry: {error}"))?;
                let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
                    continue;
                };
                let stat = match fs::read_to_string(entry.path().join("stat")) {
                    Ok(stat) => stat,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                    Err(error) => return Err(format!("inspect process {pid}: {error}")),
                };
                let fields = stat
                    .rsplit_once(')')
                    .map(|(_, fields)| fields.split_whitespace().collect::<Vec<_>>())
                    .ok_or_else(|| format!("parse process group for PID {pid}"))?;
                let state = fields.first().copied().unwrap_or_default();
                let process_group = fields
                    .get(2)
                    .and_then(|value| value.parse::<i32>().ok())
                    .ok_or_else(|| format!("parse process group for PID {pid}"))?;
                if process_group == self.0 && state != "Z" && state != "X" {
                    return Ok(false);
                }
            }
            Ok(true)
        }
        #[cfg(not(target_os = "linux"))]
        {
            let output = ProcessCommand::new("ps")
                .args(["-Ao", "pgid=,stat="])
                .output()
                .map_err(|error| format!("query process groups: {error}"))?;
            if !output.status.success() {
                return Err(format!("query process groups returned {}", output.status));
            }
            let processes = String::from_utf8_lossy(&output.stdout);
            for line in processes.lines() {
                let mut fields = line.split_whitespace();
                let Some(group) = fields.next().and_then(|value| value.parse::<i32>().ok()) else {
                    continue;
                };
                let Some(state) = fields.next() else {
                    continue;
                };
                if group == self.0 && !state.starts_with('Z') && !state.starts_with('X') {
                    return Ok(false);
                }
            }
            Ok(true)
        }
    }

    fn terminate(&mut self) {
        let result = unsafe { libc::kill(-self.0, libc::SIGKILL) };
        assert!(result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH));
    }
}

#[cfg(windows)]
struct SupervisorBoundary(WindowsSupervisorJob);

#[cfg(windows)]
impl SupervisorBoundary {
    fn assign(worker: &std::process::Child) -> Self {
        Self(WindowsSupervisorJob::assign(worker))
    }

    fn is_empty(&self) -> Result<bool, String> {
        self.0.is_empty()
    }

    fn terminate(&mut self) {
        self.0.terminate();
    }
}

#[cfg(windows)]
struct WindowsSupervisorJob(Option<std::os::windows::io::OwnedHandle>);

#[cfg(windows)]
impl WindowsSupervisorJob {
    fn assign(worker: &std::process::Child) -> Self {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::io::FromRawHandle;
        use std::os::windows::io::OwnedHandle;
        use std::os::windows::io::RawHandle;

        #[repr(C)]
        struct BasicLimitInformation {
            per_process_user_time_limit: i64,
            per_job_user_time_limit: i64,
            limit_flags: u32,
            minimum_working_set_size: usize,
            maximum_working_set_size: usize,
            active_process_limit: u32,
            affinity: usize,
            priority_class: u32,
            scheduling_class: u32,
        }
        #[repr(C)]
        struct IoCounters {
            read_operation_count: u64,
            write_operation_count: u64,
            other_operation_count: u64,
            read_transfer_count: u64,
            write_transfer_count: u64,
            other_transfer_count: u64,
        }
        #[repr(C)]
        struct ExtendedLimitInformation {
            basic_limit_information: BasicLimitInformation,
            io_info: IoCounters,
            process_memory_limit: usize,
            job_memory_limit: usize,
            peak_process_memory_used: usize,
            peak_job_memory_used: usize,
        }
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn CreateJobObjectW(attributes: *const std::ffi::c_void, name: *const u16) -> RawHandle;
            fn SetInformationJobObject(
                job: RawHandle,
                class: i32,
                information: *const std::ffi::c_void,
                length: u32,
            ) -> i32;
            fn AssignProcessToJobObject(job: RawHandle, process: RawHandle) -> i32;
            fn GetLastError() -> u32;
        }
        const JOB_OBJECT_EXTENDED_LIMIT_INFORMATION: i32 = 9;
        const JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE: u32 = 0x0000_2000;

        let raw = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        assert!(
            !raw.is_null(),
            "create independent supervisor Job Object: Windows error {}",
            unsafe { GetLastError() }
        );
        let handle = unsafe { OwnedHandle::from_raw_handle(raw) };
        let limits = ExtendedLimitInformation {
            basic_limit_information: BasicLimitInformation {
                per_process_user_time_limit: 0,
                per_job_user_time_limit: 0,
                limit_flags: JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
                minimum_working_set_size: 0,
                maximum_working_set_size: 0,
                active_process_limit: 0,
                affinity: 0,
                priority_class: 0,
                scheduling_class: 0,
            },
            io_info: IoCounters {
                read_operation_count: 0,
                write_operation_count: 0,
                other_operation_count: 0,
                read_transfer_count: 0,
                write_transfer_count: 0,
                other_transfer_count: 0,
            },
            process_memory_limit: 0,
            job_memory_limit: 0,
            peak_process_memory_used: 0,
            peak_job_memory_used: 0,
        };
        let set = unsafe {
            SetInformationJobObject(
                raw,
                JOB_OBJECT_EXTENDED_LIMIT_INFORMATION,
                (&limits as *const ExtendedLimitInformation).cast(),
                std::mem::size_of::<ExtendedLimitInformation>() as u32,
            )
        };
        assert_ne!(set, 0, "configure supervisor Job Object: Windows error {}", unsafe {
            GetLastError()
        });
        let assigned = unsafe { AssignProcessToJobObject(raw, worker.as_raw_handle()) };
        assert_ne!(
            assigned,
            0,
            "assign worker to supervisor Job Object: Windows error {}",
            unsafe { GetLastError() }
        );
        Self(Some(handle))
    }

    fn is_empty(&self) -> Result<bool, String> {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::io::RawHandle;

        const WAIT_OBJECT_0: u32 = 0;
        const WAIT_TIMEOUT: u32 = 258;
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn WaitForSingleObject(handle: RawHandle, milliseconds: u32) -> u32;
            fn GetLastError() -> u32;
        }
        let handle = self.0.as_ref().expect("supervisor Job Object should be open");
        match unsafe { WaitForSingleObject(handle.as_raw_handle(), 0) } {
            WAIT_OBJECT_0 => Ok(true),
            WAIT_TIMEOUT => Ok(false),
            _ => Err(format!("WaitForSingleObject(Job Object): Windows error {}", unsafe {
                GetLastError()
            })),
        }
    }

    fn terminate(&mut self) {
        use std::os::windows::io::AsRawHandle;
        use std::os::windows::io::RawHandle;

        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn TerminateJobObject(job: RawHandle, exit_code: u32) -> i32;
            fn GetLastError() -> u32;
        }
        let handle = self.0.as_ref().expect("supervisor Job Object should be open");
        let result = unsafe { TerminateJobObject(handle.as_raw_handle(), 1) };
        assert_ne!(result, 0, "terminate supervisor Job Object: Windows error {}", unsafe {
            GetLastError()
        });
    }
}

fn wait_for_boundary_empty(boundary: &SupervisorBoundary, timeout: Duration) -> Result<(), String> {
    let deadline = Instant::now() + timeout;
    loop {
        if boundary.is_empty()? {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(format!("supervisor process boundary remained active for {timeout:?}"));
        }
        thread::sleep(Duration::from_millis(10));
    }
}
