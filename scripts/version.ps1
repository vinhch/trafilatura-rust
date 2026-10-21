#Requires -Version 7.2

# Check the crate version in Cargo.toml: that it is a release version, and with --tag that it is the
# version the release tag names. release.yml calls the .sh twin so a tag cannot publish a different
# version.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptName = 'version.ps1'

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [--check] [--tag vX.Y.Z]

Prints the crate version from Cargo.toml.

  --check        Fail unless the version is X.Y.Z or X.Y.Z-pre (semver, no build metadata).
  --tag TAG      Fail unless TAG is "v" followed by the version. Implies --check.
  -h, --help     Show this message.
"@
}

$Check = $false
$Tag = ''
for ($Index = 0; $Index -lt $args.Count; $Index++) {
    switch ($args[$Index]) {
        { $_ -in '-h', '--help' } { Show-Usage; exit 0 }
        '--check' { $Check = $true }
        '--tag' {
            if ($Index + 1 -ge $args.Count) { Fail '--tag needs a value' }
            $Index++
            $Tag = $args[$Index]
            $Check = $true
        }
        default { Fail "unknown argument: $($args[$Index])" }
    }
}

$Manifest = Join-Path (Split-Path -Parent $PSScriptRoot) 'Cargo.toml'

# The first `version` key after [package], before the next table header.
$Version = ''
$InPackage = $false
foreach ($Line in Get-Content $Manifest) {
    if ($Line -match '^\[') { $InPackage = ($Line -eq '[package]'); continue }
    if ($InPackage -and $Line -match '^version *= *"([^"]*)"') { $Version = $Matches[1]; break }
}
if (-not $Version) { Fail "no version under [package] in $Manifest" }

if ($Check) {
    if ($Version -notmatch '^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z.-]+)?$') {
        Fail "version '$Version' is not X.Y.Z or X.Y.Z-pre"
    }
    if ($Tag -and $Tag -cne "v$Version") {
        Fail "tag '$Tag' does not match Cargo.toml version '$Version' (expected 'v$Version')"
    }
}

$Version
