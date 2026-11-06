$ErrorActionPreference = 'Stop'
$gate = Join-Path $PSScriptRoot 'assert-windows-probe-results.ps1'
$temp = Join-Path ([System.IO.Path]::GetTempPath()) ("windows-probe-gate-tests-{0}" -f [guid]::NewGuid())
New-Item -ItemType Directory -Path $temp | Out-Null

$required = @(
    'command_runner::internal::windows_cancel_interleaving_tests::test_successful_cancellation_interleaving_completes',
    'command_runner::internal::command_io::tests::test_error_not_found_requires_bounded_helper_confirmation',
    'command_runner::internal::command_io::tests::test_error_not_found_is_benign_after_helper_completion',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_probe_watchdog_reaps_blocked_child',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_real_cancel_read_to_blocked_tee_interleaving',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_successful_cancel_interleaving_read_to_tee',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_successful_cancel_interleaving_readiness',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_successful_cancel_interleaving_eof_to_flush',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_successful_cancel_interleaving_stdin_check_to_write',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_successful_cancel_interleaving_production_command_io',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_probe_controller_rejects_nonzero_child',
    'command_runner::internal::output_collector::tests::windows_probe_tests::test_windows_probe_controller_rejects_missing_completion_marker'
)

function Write-Fixture([string[]] $listEntries, [string[]] $runEntries, [string] $name) {
    $list = Join-Path $temp "$name.list"
    $run = Join-Path $temp "$name.run"
    [System.IO.File]::WriteAllLines($list, $listEntries)
    [System.IO.File]::WriteAllLines($run, $runEntries)
    return @($list, $run)
}

function Assert-Case([string] $name, [string[]] $listEntries, [string[]] $runEntries, [bool] $expected, [string] $requiredMessage = '') {
    $paths = Write-Fixture $listEntries $runEntries $name
    $succeeded = $false
    $failure = ''
    try {
        & $gate -ListPath $paths[0] -RunPath $paths[1] *> $null
        $succeeded = $true
    }
    catch { $failure = $_.ToString() }
    if ($succeeded -ne $expected) { throw "Fixture '$name' expected success=$expected, got success=$succeeded. $failure" }
    if ($requiredMessage -and $failure -notmatch [regex]::Escape($requiredMessage)) {
        throw "Fixture '$name' did not report '$requiredMessage'. Actual error: $failure"
    }
    Write-Output ("fixture={0} passed={1}" -f $name, $expected)
}

try {
    $list = @($required | ForEach-Object { "${_}: test" })
    $run = @($required | ForEach-Object { "test $_ ... ok" })
    Assert-Case -name 'complete-set' -listEntries $list -runEntries $run -expected $true
    Assert-Case -name 'extra-test-allowed' -listEntries ($list + 'some::additional_test: test') -runEntries ($run + 'test some::additional_test ... ok') -expected $true
    Assert-Case -name 'missing-discovery' -listEntries ($list | Where-Object { $_ -ne "$($required[0]): test" }) -runEntries $run -expected $false -requiredMessage $required[0]
    Assert-Case -name 'missing-run-result' -listEntries $list -runEntries ($run | Where-Object { $_ -ne "test $($required[1]) ... ok" }) -expected $false -requiredMessage $required[1]
    Assert-Case -name 'ignored' -listEntries $list -runEntries (@("test $($required[2]) ... ignored") + @($run | Where-Object { $_ -ne "test $($required[2]) ... ok" })) -expected $false -requiredMessage $required[2]
    Assert-Case -name 'failed' -listEntries $list -runEntries (@("test $($required[3]) ... FAILED") + @($run | Where-Object { $_ -ne "test $($required[3]) ... ok" })) -expected $false -requiredMessage $required[3]
    $emptyList = [string[]]@()
    Assert-Case -name 'empty-list' -listEntries $emptyList -runEntries $run -expected $false -requiredMessage $required[0]
    Assert-Case -name 'child-summary-only' -listEntries $list -runEntries @('test result: ok. 1 passed; 0 failed; 0 ignored') -expected $false -requiredMessage $required[0]
    Assert-Case -name 'duplicate-success-allowed' -listEntries $list -runEntries ($run + "test $($required[0]) ... ok") -expected $true
    $caseMutatedRun = @($run | ForEach-Object { if ($_ -eq "test $($required[5]) ... ok") { "test $($required[5].ToUpperInvariant()) ... ok" } else { $_ } })
    Assert-Case -name 'case-mutated-name-rejected' -listEntries $list -runEntries $caseMutatedRun -expected $false -requiredMessage $required[5]
    $shownBlock = @($run | Where-Object { $_ -ne "test $($required[4]) ... ok" }) + @('successes:', "    $($required[4])", 'test result: ok. 1 passed; 0 failed; 0 ignored', "test $($required[4]) ... ok")
    Assert-Case -name 'display-block-cannot-fill-top-level' -listEntries $list -runEntries $shownBlock -expected $false -requiredMessage $required[4]

    $missingList = Join-Path $temp 'missing.list'
    $existingRun = Join-Path $temp 'missing.run'
    [System.IO.File]::WriteAllLines($existingRun, $run)
    $missingFailed = $false
    try { & $gate -ListPath $missingList -RunPath $existingRun *> $null }
    catch { $missingFailed = $_.ToString().Contains('Required input file is missing') }
    if (-not $missingFailed) { throw 'Fixture missing-file did not fail with a missing-file diagnostic.' }
    Write-Output 'fixture=missing-file passed=False'
}
finally {
    Remove-Item -LiteralPath $temp -Recurse -Force
}
