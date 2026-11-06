[CmdletBinding()]
param(
    [Parameter(Mandatory = $true)]
    [string] $ListPath,

    [Parameter(Mandatory = $true)]
    [string[]] $RunPath
)

$ErrorActionPreference = 'Stop'

$requiredTests = @(
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

$errors = [System.Collections.Generic.List[string]]::new()
$discovered = [System.Collections.Generic.HashSet[string]]::new([System.StringComparer]::Ordinal)
$results = [System.Collections.Hashtable]::new([System.StringComparer]::Ordinal)

foreach ($path in (@($ListPath) + @($RunPath))) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) {
        $errors.Add("Required input file is missing: $path")
    }
}

if (Test-Path -LiteralPath $ListPath -PathType Leaf) {
    foreach ($line in [System.IO.File]::ReadAllLines((Resolve-Path -LiteralPath $ListPath))) {
        if ($line -cmatch '^(.+): test$') {
            [void] $discovered.Add($Matches[1])
        }
    }
    if ($discovered.Count -eq 0) {
        $errors.Add("Test list contains no libtest entries: $ListPath")
    }
}

foreach ($runPath in $RunPath) {
    if (-not (Test-Path -LiteralPath $runPath -PathType Leaf)) { continue }
    $topLevelResults = $true
    foreach ($line in [System.IO.File]::ReadAllLines((Resolve-Path -LiteralPath $runPath))) {
        if ($line.Trim() -ceq 'successes:' -or $line.Trim() -ceq 'failures:' -or $line -cmatch '^test result:') {
            $topLevelResults = $false
            continue
        }
        if (-not $topLevelResults) { continue }
        if ($line -cmatch '^test (.+) \.\.\. (ok|FAILED|ignored)(?: .*)?$') {
            $testName = $Matches[1]
            $status = $Matches[2]
            if (-not $results.ContainsKey($testName)) {
                $results[$testName] = [System.Collections.Generic.List[string]]::new()
            }
            $results[$testName].Add($status)
        }
    }
}

foreach ($testName in $requiredTests) {
    $isDiscovered = $discovered.Contains($testName)
    $statuses = @()
    if ($results.ContainsKey($testName)) { $statuses = @($results[$testName].ToArray()) }
    $passed = $isDiscovered -and ($statuses -ccontains 'ok') -and -not ($statuses -ccontains 'FAILED') -and -not ($statuses -ccontains 'ignored')
    $record = [ordered]@{
        event = 'required_test_result'
        test = $testName
        discovered = [bool] $isDiscovered
        passed = [bool] $passed
    }
    $record | ConvertTo-Json -Compress
    if (-not $isDiscovered) { $errors.Add("Required test was not discovered: $testName") }
    if ($statuses.Count -eq 0) { $errors.Add("Required test has no top-level run result: $testName") }
    if ($statuses -ccontains 'ignored') { $errors.Add("Required test was ignored: $testName") }
    if ($statuses -ccontains 'FAILED') { $errors.Add("Required test failed: $testName") }
    if ($statuses.Count -gt 0 -and -not ($statuses -ccontains 'ok')) { $errors.Add("Required test has no successful result: $testName") }
}

if ($errors.Count -gt 0) {
    foreach ($message in $errors) { [Console]::Error.WriteLine($message) }
    throw (("Windows probe result identity gate failed with {0} error(s):" -f $errors.Count) + [Environment]::NewLine + ($errors -join [Environment]::NewLine))
}

[ordered]@{ event = 'passed'; required_test_count = $requiredTests.Count } | ConvertTo-Json -Compress
