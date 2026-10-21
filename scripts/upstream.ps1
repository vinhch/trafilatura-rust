#Requires -Version 7.2

# Show what changed in upstream Python trafilatura since the release this port follows, as the first
# step of the sync procedure in AGENTS.md. Read-only: it changes neither checkout.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$ScriptName = 'upstream.ps1'

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [--upstream PATH] [--to REF] [--patch]

Reads the pinned upstream version and commit from Cargo.toml [package.metadata.upstream], then
prints, for the upstream checkout at PATH, the commits and the per-file change summary between the
pin and REF under trafilatura/ and tests/.

  --upstream PATH  Upstream git checkout. Default: `$env:TRAFILATURA_UPSTREAM, else ../trafilatura.
  --to REF         Target ref. Default: the highest v* tag in the checkout.
  --patch          Print the full diff of trafilatura/ instead of the summary.
  -h, --help       Show this message.

Fetch tags in the upstream checkout first (git -C PATH fetch --tags); this script never fetches.
"@
}

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Upstream = if ($env:TRAFILATURA_UPSTREAM) { $env:TRAFILATURA_UPSTREAM } else { Join-Path $RepoRoot '../trafilatura' }
$Target = ''
$Patch = $false
for ($Index = 0; $Index -lt $args.Count; $Index++) {
    switch ($args[$Index]) {
        { $_ -in '-h', '--help' } { Show-Usage; exit 0 }
        '--upstream' {
            if ($Index + 1 -ge $args.Count) { Fail '--upstream needs a value' }
            $Index++
            $Upstream = $args[$Index]
        }
        '--to' {
            if ($Index + 1 -ge $args.Count) { Fail '--to needs a value' }
            $Index++
            $Target = $args[$Index]
        }
        '--patch' { $Patch = $true }
        default { Fail "unknown argument: $($args[$Index])" }
    }
}

# Keys of [package.metadata.upstream], before the next table header.
$Pin = @{}
$InTable = $false
foreach ($Line in Get-Content (Join-Path $RepoRoot 'Cargo.toml')) {
    if ($Line -match '^\[') { $InTable = ($Line -eq '[package.metadata.upstream]'); continue }
    if ($InTable -and $Line -match '^(\w+) *= *"([^"]*)"') { $Pin[$Matches[1]] = $Matches[2] }
}
if (-not $Pin['version'] -or -not $Pin['commit']) {
    Fail 'Cargo.toml has no version and commit under [package.metadata.upstream]'
}
$PinnedCommit = $Pin['commit']

git -C $Upstream rev-parse --git-dir *> $null
if ($LASTEXITCODE -ne 0) { Fail "$Upstream is not a git checkout" }
git -C $Upstream cat-file -e "$PinnedCommit^{commit}" *> $null
if ($LASTEXITCODE -ne 0) { Fail "pinned commit $PinnedCommit is not in $Upstream; fetch it first" }

if (-not $Target) {
    $Target = git -C $Upstream tag --list 'v*' --sort=-v:refname | Select-Object -First 1
    if (-not $Target) { Fail "no v* tag in $Upstream; fetch tags or pass --to" }
}
$TargetCommit = git -C $Upstream rev-parse --verify "$Target^{commit}" 2> $null
if ($LASTEXITCODE -ne 0) { Fail "ref '$Target' does not resolve in $Upstream" }

"pinned:  v$($Pin['version'])  $PinnedCommit"
"target:  $Target  $TargetCommit"
if ($PinnedCommit -eq $TargetCommit) {
    'up to date'
    exit 0
}
git -C $Upstream merge-base --is-ancestor $TargetCommit $PinnedCommit
if ($LASTEXITCODE -eq 0) { "note: $Target is older than the pin; the summary shows changes in reverse" }

if ($Patch) {
    git -C $Upstream --no-pager diff $PinnedCommit $TargetCommit -- trafilatura/
    exit 0
}

''
'commits:'
git -C $Upstream --no-pager log --oneline --no-merges "$PinnedCommit..$TargetCommit" -- trafilatura/ tests/
''
'files:'
git -C $Upstream --no-pager diff --stat=120 $PinnedCommit $TargetCommit -- trafilatura/ tests/
