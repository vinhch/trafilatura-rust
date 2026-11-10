#Requires -Version 7.2

# Runs scripts/pin-check.ps1 on generated manifests, one per case: an exact requirement in each dependency
# table form (string, inline table, spaced, dotted key, dev, build, target, sub-table), requirements and
# package keys that are not exact, comments and other tables, the repo Cargo.toml, a missing file and the
# -h and unknown-flag exits. Manifests are written under .temp/pin-check-test/ on every run.
# pin-check-test.sh is the twin.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptName = 'pin-check-test.ps1'

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName

Runs scripts/pin-check.ps1 on generated manifests and checks the exit code and messages of each case.

  -h, --help   Show this message.
"@
}

if ($args.Count -gt 0) {
    switch -CaseSensitive ($args[0]) {
        { $_ -cin '-h', '--help' } { Show-Usage; exit 0 }
        default { Fail "unknown argument: $($args[0])" }
    }
}

$RepoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $RepoRoot

$WorkRel = '.temp/pin-check-test'
$WorkAbs = Join-Path $RepoRoot $WorkRel
if (Test-Path -LiteralPath $WorkAbs) { Remove-Item -LiteralPath $WorkAbs -Recurse -Force }
New-Item -ItemType Directory -Force -Path $WorkAbs | Out-Null

$Passes = 0
$Failures = 0

# New-Manifest <name> <lines...>: writes the manifest of a case to $WorkAbs/<name>.toml.
function New-Manifest {
    param([string]$Name, [string[]]$Lines)
    [IO.File]::WriteAllText((Join-Path $WorkAbs "$Name.toml"), (($Lines -join "`n") + "`n"))
}

# Invoke-Case <name> <expected exit> <expected stderr fragment or empty> <args...>: runs the script in its own
# process and checks the exit code and that stderr holds the fragment (empty: stderr must be empty).
function Invoke-Case {
    param([string]$Name, [int]$WantExit, [string]$WantErr, [string[]]$Files = @())
    $OutFile = Join-Path $WorkAbs "out-$Name"
    $ErrFile = Join-Path $WorkAbs "err-$Name"
    $Argv = @('-NoProfile', '-File', (Join-Path $RepoRoot 'scripts/pin-check.ps1')) + $Files
    $Proc = Start-Process -FilePath 'pwsh' -ArgumentList $Argv -NoNewWindow -Wait -PassThru `
        -RedirectStandardOutput $OutFile -RedirectStandardError $ErrFile
    $GotErr = [IO.File]::ReadAllText($ErrFile)
    $Problems = [Collections.Generic.List[string]]::new()
    if ($Proc.ExitCode -ne $WantExit) { $Problems.Add("exit $($Proc.ExitCode), want $WantExit") }
    if ($WantErr) {
        if (-not $GotErr.Contains($WantErr)) { $Problems.Add("stderr lacks '$WantErr'") }
    } elseif ($GotErr.Length -gt 0) {
        $Problems.Add("stderr '$($GotErr.TrimEnd())', want none")
    }
    if ($Problems.Count -eq 0) {
        $script:Passes++
    } else {
        $Host.UI.WriteErrorLine("pin-check-test.ps1: case $($Name): $($Problems -join '; ')")
        $script:Failures++
    }
}

# Test-Exact <name> <line number of the hit> <manifest lines...>: the hit is reported as
# <manifest>:<line>: exact requirement.
function Test-Exact {
    param([string]$Name, [int]$Line, [string[]]$Lines)
    New-Manifest $Name $Lines
    Invoke-Case $Name 1 "pin-check.ps1: $WorkRel/$Name.toml:$($Line): exact requirement: " @("$WorkRel/$Name.toml")
}

Test-Exact 'exact_string' 5 @('[package]', 'name = "x"', '', '[dependencies]', 'foo = "=1.2"')
Test-Exact 'exact_table' 5 @('[package]', 'name = "x"', '', '[dependencies]', 'foo = { version = "=1.2", default-features = false }')
Test-Exact 'exact_table_later_key' 5 @('[package]', 'name = "x"', '', '[dependencies]', 'foo = { path = "../foo", version = "=1.2" }')
Test-Exact 'exact_spaced' 5 @('[package]', 'name = "x"', '', '[dependencies]', 'foo = "= 1.2"')
Test-Exact 'exact_single_quote' 5 @('[package]', 'name = "x"', '', '[dependencies]', "foo = '=1.2'")
Test-Exact 'exact_dotted_key' 5 @('[package]', 'name = "x"', '', '[dependencies]', 'foo.version = "=1.2"')
Test-Exact 'exact_dev' 5 @('[package]', 'name = "x"', '', '[dev-dependencies]', 'foo = "=1.2"')
Test-Exact 'exact_build' 5 @('[package]', 'name = "x"', '', '[build-dependencies]', 'foo = "=1.2"')
Test-Exact 'exact_target' 5 @('[package]', 'name = "x"', '', "[target.'cfg(unix)'.dependencies]", 'foo = "=1.2"')
Test-Exact 'exact_target_dev' 5 @('[package]', 'name = "x"', '', "[target.'cfg(unix)'.dev-dependencies]", 'foo = "=1.2"')
Test-Exact 'exact_subtable' 5 @('[package]', 'name = "x"', '', '[dependencies.foo]', 'version = "=1.2"')
Test-Exact 'exact_after_ok_lines' 8 @('[package]', 'name = "x"', '', '[dependencies]', 'bar = "1"', '# foo = "=0.1"', 'baz = { version = "2" }', 'foo = "=1.2"')

# A UTF-8 BOM before the first header: cargo reads the manifest, so the check must too.
[IO.File]::WriteAllText((Join-Path $WorkAbs 'bom_header.toml'), ([string][char]0xFEFF + "[dependencies]`nfoo = `"=1.2`"`n"))
Invoke-Case 'bom_header' 1 "pin-check.ps1: $WorkRel/bom_header.toml:2: exact requirement: " @("$WorkRel/bom_header.toml")

# An absolute manifest path.
Invoke-Case 'absolute_path' 1 "pin-check.ps1: $WorkAbs/exact_string.toml:5: exact requirement: " @("$WorkAbs/exact_string.toml")

New-Manifest 'caret_ok' @('[package]', 'name = "x"', '', '[dependencies]', 'foo = "1.2"', 'bar = { version = ">=1, <2" }',
    'baz = { version = "~1.2", optional = true }', '[dependencies.qux]', 'version = "1"', 'features = ["=x"]')
Invoke-Case 'caret_ok' 0 '' @("$WorkRel/caret_ok.toml")

New-Manifest 'package_keys_ok' @('[package]', 'name = "x"', 'version = "0.1.0"', 'rust-version = "1.98.1"', '',
    '[package.metadata.upstream]', 'version = "2.3.1"', 'commit = "e2f85c965662c7df26b73290ef175b2c2a5fdca6"')
Invoke-Case 'package_keys_ok' 0 '' @("$WorkRel/package_keys_ok.toml")

New-Manifest 'other_tables_ok' @('[package]', 'name = "x"', '', '[features]', 'foo = ["=x"]', '', '[package.metadata.docs.rs]',
    'version = "=1"', '', '[dependencies]', 'bar = "1"', '', '[lints.rust]', 'version = "=1"')
Invoke-Case 'other_tables_ok' 0 '' @("$WorkRel/other_tables_ok.toml")

New-Manifest 'comment_ok' @('[dependencies]', '# foo = "=1.2"', '   # bar = { version = "=1" }', 'baz = "1"')
Invoke-Case 'comment_ok' 0 '' @("$WorkRel/comment_ok.toml")

Invoke-Case 'real_manifest' 0 '' @('Cargo.toml')
Invoke-Case 'default_manifest' 0 ''
Invoke-Case 'help' 0 '' @('-h')
Invoke-Case 'unknown_flag' 1 'unknown argument: --bogus' @('--bogus')
Invoke-Case 'missing_manifest' 1 "$WorkRel/nope.toml: no such file" @("$WorkRel/nope.toml")

# -h prints the usage on stdout.
$HelpFirst = (([IO.File]::ReadAllText((Join-Path $WorkAbs 'out-help'))) -split "`n")[0]
if ($HelpFirst.StartsWith('Usage:', [StringComparison]::Ordinal)) {
    $Passes++
} else {
    $Host.UI.WriteErrorLine("pin-check-test.ps1: case help: first stdout line '$HelpFirst', want prefix 'Usage:'")
    $Failures++
}

if ($Failures -gt 0) { exit 1 }
"pin-check-test: $Passes cases passed"
