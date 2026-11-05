#Requires -Version 7.2

# Check hand-written .rs files against the file size guide in AGENTS.md (File size guide): 800 lines for a
# production file, 1,000 for a test file, and 1.5 times either as a hard cap. Past the guide a file needs a
# size: line in its leading block. Generated files (@generated on line 1) are skipped.
# size-check.sh is the twin: same output and exit codes; change both together.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$ScriptName = 'size-check.ps1'

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [FILE...]

Checks the .rs files git tracks, or the FILEs given (relative to the repo root), against the file size guide
in AGENTS.md. Exits 1 on any violation.

  -h, --help   Show this message.
"@
}

if ($args.Count -gt 0) {
    switch -CaseSensitive ($args[0]) {
        { $_ -cin '-h', '--help' } { Show-Usage; exit 0 }
        { $_ -clike '-*' } { Fail "unknown argument: $_" }
    }
}

function Remove-Cr {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string]$Line)
    if ($Line.EndsWith("`r", [StringComparison]::Ordinal)) { return $Line.Substring(0, $Line.Length - 1) }
    return $Line
}

# The record count of the awk twin: the newline count, plus one for a last line without a newline.
function Get-FileStats {
    param([Parameter(Mandatory = $true)][string]$Path)
    $Text = [Text.Encoding]::UTF8.GetString([IO.File]::ReadAllBytes($Path))
    if ($Text.Length -eq 0) {
        return [pscustomobject]@{ Lines = 0; Generated = $false; Reason = $false }
    }
    $Split = $Text -csplit "`n"
    $Lines = if ($Text.EndsWith("`n", [StringComparison]::Ordinal)) { $Split.Count - 1 } else { $Split.Count }
    $Generated = (Remove-Cr ($Split[0])).Contains('@generated')

    $OpenAttr = $false
    $HasModDoc = $false
    $DocReason = $false
    $SlashReason = $false
    foreach ($Raw in $Split) {
        $Trim = (Remove-Cr $Raw).TrimStart(' ')
        if ($OpenAttr) {
            if ($Trim -cmatch '\]$') { $OpenAttr = $false }
        } elseif ($Trim -ceq '' -or $Trim -cmatch '^(//|#!\[)') {
            if ($Trim -cmatch '^#!\[' -and $Trim -cnotmatch '\]$') { $OpenAttr = $true }
        } else {
            break
        }
        if ($Trim -cmatch '^//!') { $HasModDoc = $true }
        if ($Trim -cmatch '^//! size: [^ \t\n\v\f\r]') { $DocReason = $true }
        if ($Trim -cmatch '^// size: [^ \t\n\v\f\r]') { $SlashReason = $true }
    }
    $Reason = $DocReason -or ($SlashReason -and -not $HasModDoc)
    return [pscustomobject]@{ Lines = $Lines; Generated = $Generated; Reason = $Reason }
}

Set-Location (Split-Path -Parent $PSScriptRoot)

if ($args.Count -gt 0) {
    $Paths = @($args)
} else {
    $Listed = & git ls-files -- '*.rs'
    if ($LASTEXITCODE -ne 0) { Fail "git ls-files exited with $LASTEXITCODE" }
    $Paths = @($Listed | Where-Object { $_ })
}

$Checked = 0
$GeneratedCount = 0
$Violations = 0
foreach ($Path in $Paths) {
    $Full = if ([IO.Path]::IsPathRooted($Path)) { $Path } else { Join-Path (Get-Location).ProviderPath $Path }
    if (-not [IO.File]::Exists($Full)) {
        $Host.UI.WriteErrorLine("$ScriptName`: $($Path): no such file")
        $Violations++
        continue
    }
    $Stats = Get-FileStats $Full
    if ($Stats.Generated) {
        $GeneratedCount++
        continue
    }
    $Checked++
    if ($Path -cmatch '(^|/)tests(/|\.rs$)') { $Kind = 'test' } else { $Kind = 'production' }
    if ($Kind -ceq 'test') { $Cap = 1000 } else { $Cap = 800 }
    $Hard = [int]($Cap * 3 / 2)
    if ($Stats.Lines -gt $Hard) {
        $Host.UI.WriteErrorLine("$ScriptName`: $($Path): $($Stats.Lines) lines, over the $Hard-line $Kind hard cap")
        $Violations++
    } elseif ($Stats.Lines -gt $Cap -and -not $Stats.Reason) {
        $Host.UI.WriteErrorLine("$ScriptName`: $($Path): $($Stats.Lines) lines, over the $Cap-line $Kind cap without a size: line")
        $Violations++
    }
}

if ($Violations -gt 0) { exit 1 }
if ($Checked -eq 0) { Fail 'no hand-written .rs file checked' }
"size-check: $Checked files checked, $GeneratedCount generated skipped"
