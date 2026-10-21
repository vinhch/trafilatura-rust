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

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [--no-deny]

Runs scripts/version.ps1 --check, cargo fmt --check,
cargo clippy --locked --all-targets --all-features -- -D warnings, cargo test --locked --all-features (doc tests included) and cargo deny check.

  --no-deny    Skip cargo deny, for a machine without it installed. CI never passes this.
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
foreach ($Argument in $args) {
    switch ($Argument) {
        { $_ -in '-h', '--help' } { Show-Usage; exit 0 }
        '--no-deny' { $RunDeny = $false }
        default { Fail "unknown argument: $Argument" }
    }
}

Set-Location (Split-Path -Parent $PSScriptRoot)

# version.ps1 calls exit only on failure, so reset the code it would leave unset on success.
$global:LASTEXITCODE = 0
& (Join-Path $PSScriptRoot 'version.ps1') --check
if ($global:LASTEXITCODE -ne 0) { Fail "'version.ps1 --check' exited with $global:LASTEXITCODE" }
Invoke-Checked @('cargo', 'fmt', '--check')
Invoke-Checked @('cargo', 'clippy', '--locked', '--all-targets', '--all-features', '--', '-D', 'warnings')
Invoke-Checked @('cargo', 'test', '--locked', '--all-features')
if ($RunDeny) {
    # Asked through cargo, which also finds subcommands in ~/.cargo/bin when that is not on PATH.
    cargo deny --version *> $null
    if ($LASTEXITCODE -ne 0) {
        Fail 'cargo-deny is not installed (cargo install cargo-deny, or pass --no-deny)'
    }
    Invoke-Checked @('cargo', 'deny', 'check')
}
