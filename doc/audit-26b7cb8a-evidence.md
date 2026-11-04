# CMD-AUDIT-002 I/O helper failure evidence

## Scope and environment

- Project: `rs-command` at `/data/working/qubit/rust-common/.worktrees/rs-command/cmd-audit-20261104`.
- Platform: Linux x86_64 (`x86_64-unknown-linux-gnu`).
- Rust: `rustc 1.94.0 (4a4ef493e 2026-03-02)`.
- Base source revision: `92cb029945d3d4bab054b43d320fe379b740d1cc`.
- Dirty source diff SHA-256 at the time of the final focused run and CI run: `41ec69d197aa4f620a5f3a410bf7e5baf763aac37ae1f88697642f78e19c6789` (`git diff --binary -- src/command_runner/internal/running_command.rs | sha256sum`).
- The probes use an injected `OutputReader` that returns `OutputCaptureError::Read` with partial stdout and a test-only `ChildWrapper` controlled by a condition variable. This is a private state-machine seam; it is not a real child process or production pipe failure.

## Probe results

Command: `cargo test --lib test_audit_002 -- --nocapture`

- Initial attempt: exit 101; 1 passed, 1 failed. The no-timeout probe's completion reader waited for cancellation, while the simulated child remained live and the runner had no timeout. This was a test-seam deadlock, not evidence of a production failure. Replaced that companion reader with an immediately completed successful reader so the probe isolates the failing stdout helper.
- Final run: exit 0; 2 passed, 0 failed, 99 filtered out; 2.00 s.

### Finite timeout

`test_audit_002_read_failure_while_child_alive_with_timeout` synchronizes reader failure and timer registration over channels, advances `ManualMonotonicClock` to the configured deadline, and observes completion without invoking its watchdog release. The recorded event order is `reader-failed`, `kill`, `wait`, `returned`. The result is `TimedOut`, carries `partial stdout` with `stdout_complete == false`, and retains one reader cleanup failure. This confirms the existing timeout decision wins while the simulated child is still live; the reader error is collected during cleanup.

### No timeout

`test_audit_002_read_failure_while_child_alive_without_timeout` waits for the reader-failure signal, then observes no runner result for the bounded 2-second watchdog interval. The test releases the simulated child, joins the runner, and observes `ReadOutputFailed` with `partial stdout` and `stdout_complete == false`. The watchdog release precedes runner return. This confirms that the no-timeout path blocks on child wait and does not independently wake on helper failure in this injected state.

All test threads are joined. The no-timeout child is released by the watchdog; the finite-timeout child is released by the normal termination path. No test process is left blocked.

## Reachability and classification

The test proves only the internal state-machine behavior when an output reader returns an error while a simulated child is live. It does **not** establish that ordinary production child pipes can fail this way, nor that a production child can remain live after such a failure under the supported operating systems. No production trigger was constructed, consistent with the bounded-probe design. Therefore the classification is: **state-machine behavior verified; production trigger unconfirmed**. No production behavior change is justified by this evidence, and this result must not be described as proof of production safety or as a fixed production hang.

Existing behavior tests cover related independent contracts, including cancellation without timeout (`tests/command_runner_tests.rs::test_runner_cancellation_without_timeout_terminates_running_command`), injected timeout timer (`test_runner_timeout_uses_injected_manual_timer`), and tee output (`test_command_runner_run_tees_output_to_files`, plus `tests/command_runner/internal/output_tee_tests.rs`). They do not make the injected read error a production-reachable event.

## Integration verification

Commands and results on Linux x86_64:

| Command | Result |
| --- | --- |
| `./.infra/bin/align-ci.sh` (after initial probe implementation) | Exit 0; repository style alignment and its clippy/check step succeeded. |
| `cargo test --lib test_audit_002 -- --nocapture` (after review fixes) | Exit 0; 2 passed, 0 failed, 99 filtered. |
| `git diff --check` | Exit 0. |
| `./.infra/bin/align-ci.sh` (after review fixes) | Exit 0; repository style alignment and its clippy/check step succeeded. |
| `./.infra/bin/ci-check.sh` (final run after review fixes) | Exit 0; all configured CI tasks passed: style, clippy, verify/build, strict docs/Rustdoc, README version check, release build, feature matrix, project hook, package verification, coverage, and security audit. |

The CI project hook ran `cargo test --test documentation_examples_tests` (11 passed) and `cargo bench --bench command_runner -- silent_command_runner --test` (passed). The normal and feature-matrix test suites also ran the documentation example tests (11 passed), `documentation_version_tests` (1 passed), and doctests (17 passed). Final coverage thresholds passed: functions 97.15% (545/561, required at least 95%), lines 97.07% (5044/5196, required over 90%), and regions 97.11% (7468/7690, required over 85%). The CI audit completed with exit 0. No production/platform behavior changed, so no cross-platform behavioral claim is made; the execution evidence is Linux-only.

The initial focused run's single failure was the test-seam deadlock described above. An independent review then identified two test reliability gaps: cleanup before assertion on timer-registration timeout, and lack of synchronization proving the no-timeout runner had entered child wait before its watchdog interval. Both were corrected; the focused command and full CI were rerun successfully. No CI failure remains unresolved.
