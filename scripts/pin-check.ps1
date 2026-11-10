#Requires -Version 7.2

# Fail on a dependency requirement that starts with `=` in Cargo.toml: an exact pin keeps a dependency at one
# version and blocks the security fixes Dependabot and cargo update would bring. Reads [dependencies],
# [dev-dependencies], [build-dependencies], their [target.<spec>.*] forms and [dependencies.<name>] sub-tables;
# other tables (package, metadata, features) are never checked. A requirement is a bare string value, a
# version = inside an inline table, or a version line of a sub-table. Out of scope, as none is in the repo and
# neither cargo nor Dependabot writes them: multi-line inline tables, multi-line strings, quoted table names
# and keys, and spaces around the dot of a header.
# pin-check.sh is the twin: same output and exit codes; change both together.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptName = 'pin-check.ps1'

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [MANIFEST]

Checks the dependency requirements of MANIFEST (relative to the repo root, default Cargo.toml) and exits 1
when one starts with "=".

  -h, --help   Show this message.
"@
}

if ($args.Count -gt 0) {
    switch -CaseSensitive ($args[0]) {
        { $_ -cin '-h', '--help' } { Show-Usage; exit 0 }
        { $_ -clike '-*' } { Fail "unknown argument: $_" }
    }
    if ($args.Count -gt 1) { Fail "unknown argument: $($args[1])" }
}

Set-Location (Split-Path -Parent $PSScriptRoot)

$Manifest = if ($args.Count -gt 0) { $args[0] } else { 'Cargo.toml' }
$Full = if ([IO.Path]::IsPathRooted($Manifest)) { $Manifest } else { Join-Path (Get-Location).ProviderPath $Manifest }
if (-not [IO.File]::Exists($Full)) { Fail "$($Manifest): no such file" }

$Quote = '["''][ \t]*='
$Hits = 0
$Mode = ''
$Number = 0
foreach ($Raw in ([IO.File]::ReadAllText($Full) -csplit "`n")) {
    $Number++
    $Text = $Raw.TrimEnd("`r").TrimStart(' ', "`t")
    if ($Text.StartsWith('[[', [StringComparison]::Ordinal)) { $Mode = ''; continue }
    if ($Text.StartsWith('[', [StringComparison]::Ordinal)) {
        $Table = $Text -creplace '^\[[ \t]*', '' -creplace '[ \t]*\][ \t]*(#.*)?$', ''
        $Mode = ''
        if ($Table -cmatch '(^|\.)(dev-|build-)?dependencies$') { $Mode = 'plain' }
        elseif ($Table -cmatch '(^|\.)(dev-|build-)?dependencies\.[^.]+$') { $Mode = 'sub' }
        continue
    }
    if ($Text.StartsWith('#', [StringComparison]::Ordinal) -or $Mode -eq '') { continue }
    if ($Mode -eq 'sub') {
        $Hit = $Text -cmatch "^version[ \t]*=[ \t]*$Quote"
    } else {
        $Hit = ($Text -cmatch "^[A-Za-z0-9_-]+[ \t]*=[ \t]*$Quote") -or
            ($Text -cmatch "^[A-Za-z0-9_-]+\.version[ \t]*=[ \t]*$Quote") -or
            ($Text -cmatch "[{,][ \t]*version[ \t]*=[ \t]*$Quote")
    }
    if ($Hit) {
        $Host.UI.WriteErrorLine("$ScriptName`: $($Manifest):$($Number): exact requirement: $Text")
        $Hits++
    }
}

if ($Hits -gt 0) { exit 1 }
"pin-check: $($Manifest): no exact requirement"
