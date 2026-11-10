#Requires -Version 7.2

# Run every check CI runs, with the same commands, so a developer and a workflow run one thing.
# Fails on the first failing command.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# PowerShell 7.4+ can turn a native command's stderr into a terminating error, and cargo writes its
# progress there.
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$ScriptName = 'test.ps1'

# Tests that prove recursion fits a 512 KiB stack (AGENTS.md, Runtime rules). A new one goes on this list.
$StackTests = @(
    'baseline::tests::deep_json_on_small_stack'
    'dom::tests::iter::deep_chain_walks_iteratively'
    'extraction::tests::caller_tree::deep_caller_tree_on_small_stack'
    'main_extractor::tests::dispatch::deep_nesting_fits_512k_stack'
    'pyjson::tests::deep_document_drops_on_small_stack'
    'xml::tests::output_deep_nesting_fits_512k_stack'
    'xml::tests::remove_blank_text_deep_nesting_fits_512k_stack'
    'options_discard::test_extract_deep_caller_supplied_tree'
    'api::tests::stack::extract_deep_html_fits_512k_stack'
    'api::tests::stack::bare_extraction_deep_html_fits_512k_stack'
    'api::tests::stack::html2txt_deep_html_fits_512k_stack'
)

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [--no-deny] [--report]

Runs scripts/version.ps1 --check, the pin check (scripts/pin-check.ps1), the size check (scripts/size-check.ps1), cargo fmt --check,
cargo clippy --locked --all-targets --all-features -- -D warnings,
cargo clippy --locked -- -D warnings and with --no-default-features (the library alone, which warns about
items only the tests use), cargo nextest run --locked --all-features plus cargo test --locked --all-features --doc
(nextest runs no doc tests), the batch example (examples/batch), the stack tests again
in the dev profile (opt-level 0, a real debug build), cargo publish --dry-run --locked --all-features --allow-dirty
(packs the files the include list in Cargo.toml selects, builds the library from them and reads the crates.io
index, all without uploading) and cargo deny check.

  --no-deny    Skip cargo deny, for a machine without it installed. CI never passes this.
  --report     Run nextest with its ci profile, which writes target/nextest/ci/junit.xml for the CI test report.
  -h, --help   Show this message.
"@
}

function Invoke-Checked {
    param([Parameter(Mandatory = $true)][string[]]$Command)
    & $Command[0] $Command[1..($Command.Length - 1)]
    if ($LASTEXITCODE -ne 0) {
        Fail "'$($Command -join ' ')' exited with $LASTEXITCODE"
    }
}

$RunDeny = $true
$Report = $false
foreach ($Argument in $args) {
    switch ($Argument) {
        { $_ -in '-h', '--help' } { Show-Usage; exit 0 }
        '--no-deny' { $RunDeny = $false }
        '--report' { $Report = $true }
        default { Fail "unknown argument: $Argument" }
    }
}

Set-Location (Split-Path -Parent $PSScriptRoot)

# version.ps1 calls exit only on failure, so reset the code it would leave unset on success.
$global:LASTEXITCODE = 0
& (Join-Path $PSScriptRoot 'version.ps1') --check
if ($global:LASTEXITCODE -ne 0) { Fail "'version.ps1 --check' exited with $global:LASTEXITCODE" }
# The pin-check scripts also call exit only on failure, so each code is reset before its call.
$global:LASTEXITCODE = 0
& (Join-Path $PSScriptRoot 'pin-check-test.ps1')
if ($global:LASTEXITCODE -ne 0) { Fail "'pin-check-test.ps1' exited with $global:LASTEXITCODE" }
$global:LASTEXITCODE = 0
& (Join-Path $PSScriptRoot 'pin-check.ps1')
if ($global:LASTEXITCODE -ne 0) { Fail "'pin-check.ps1' exited with $global:LASTEXITCODE" }
# The size-check scripts also call exit only on failure, so each code is reset before its call.
$global:LASTEXITCODE = 0
& (Join-Path $PSScriptRoot 'size-check-test.ps1')
if ($global:LASTEXITCODE -ne 0) { Fail "'size-check-test.ps1' exited with $global:LASTEXITCODE" }
$global:LASTEXITCODE = 0
& (Join-Path $PSScriptRoot 'size-check.ps1')
if ($global:LASTEXITCODE -ne 0) { Fail "'size-check.ps1' exited with $global:LASTEXITCODE" }
Invoke-Checked @('cargo', 'fmt', '--check')
Invoke-Checked @('cargo', 'clippy', '--locked', '--all-targets', '--all-features', '--', '-D', 'warnings')
Invoke-Checked @('cargo', 'clippy', '--locked', '--', '-D', 'warnings')
Invoke-Checked @('cargo', 'clippy', '--locked', '--no-default-features', '--', '-D', 'warnings')
# Asked through cargo, which also finds subcommands in ~/.cargo/bin when that is not on PATH.
cargo nextest --version *> $null
if ($LASTEXITCODE -ne 0) {
    Fail 'cargo-nextest is not installed (cargo install cargo-nextest --locked)'
}
$NextestArgs = @('nextest', 'run', '--locked', '--all-features')
if ($Report) { $NextestArgs += @('--profile', 'ci') }
Invoke-Checked (@('cargo') + $NextestArgs)
# nextest runs no doc tests.
Invoke-Checked @('cargo', 'test', '--locked', '--all-features', '--doc')
# The batch example runs the public API over the committed pages in examples/batch (docs/batch.md).
Invoke-Checked @('cargo', 'run', '--locked', '--example', 'batch', '--', 'examples/batch')
# The test profile builds at opt-level 1; the dev profile stays at 0, where stack frames are largest.
# A filter that matches nothing would pass vacuously, so the selection is counted first. An #[ignore]d stack test still runs here.
$StackArgs = @('test', '--locked', '--profile', 'dev', '--all-features', '--lib', '--test', 'unit_tests', '--', '--include-ignored', '--exact') + $StackTests
$StackListing = & cargo @StackArgs --list
if ($LASTEXITCODE -ne 0) { Fail "'cargo $($StackArgs -join ' ') --list' exited with $LASTEXITCODE" }
$StackFound = @($StackListing | Where-Object { $_ -match ': test$' }).Count
if ($StackFound -ne $StackTests.Count) { Fail "stack tests: expected $($StackTests.Count), found $StackFound" }
Invoke-Checked (@('cargo') + $StackArgs)
# The dry run packages, verifies the build from the packaged files only (so a source file left out of `include` fails here)
# and reads the crates.io index, all without uploading.
Invoke-Checked @('cargo', 'publish', '--dry-run', '--locked', '--all-features', '--allow-dirty')
if ($RunDeny) {
    # Asked through cargo, which also finds subcommands in ~/.cargo/bin when that is not on PATH.
    cargo deny --version *> $null
    if ($LASTEXITCODE -ne 0) {
        Fail 'cargo-deny is not installed (cargo install cargo-deny, or pass --no-deny)'
    }
    Invoke-Checked @('cargo', 'deny', 'check')
}
