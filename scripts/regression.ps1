#Requires -Version 7.2

# Compare the sample's extraction heap and bench times of a base commit with the working tree, as the pull
# request job (bench.yml) does. The base is built in a detached worktree at .temp/regression/base-src, both
# sides run the heap sample (tests/measure_heap.rs, TRAFILATURA_MEASURE_ONLY=@sample) and the criterion bench
# (benches/extract.rs), and tools/parity (parity.measure --compare) prints the Markdown summary and applies
# the heap gate. Needs the fixtures (scripts/fixtures.ps1) and the upstream checkout. Takes several minutes:
# four cargo runs, and the first build of the bench dependencies.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$ScriptName = 'regression.ps1'

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [--base REF] [--upstream PATH]

Measures the working tree and REF with the same two commands, writes both into .temp/regression/{base,head}/,
then prints the comparison (also appended to `$env:GITHUB_STEP_SUMMARY when set). Exits 1 when the summed heap
numbers grow by more than the gate in tools/parity/parity/measure.py. When the measuring code (tests/measure,
tests/measure_heap.rs, tests/common, benches) or the upstream pin differs between REF and the working tree, the
two sides would not measure alike: only the head is measured and listed, and the exit code is 0.

  --base REF       Commit to compare with. Default: main.
  --upstream PATH  Upstream git checkout. Default: `$env:TRAFILATURA_UPSTREAM, else ../trafilatura.
  -h, --help       Show this message.
"@
}

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Upstream = if ($env:TRAFILATURA_UPSTREAM) { $env:TRAFILATURA_UPSTREAM } else { Join-Path $RepoRoot '../trafilatura' }
$Base = 'main'
for ($Index = 0; $Index -lt $args.Count; $Index++) {
    switch ($args[$Index]) {
        { $_ -in '-h', '--help' } { Show-Usage; exit 0 }
        '--base' {
            if ($Index + 1 -ge $args.Count) { Fail '--base needs a value' }
            $Index++
            $Base = $args[$Index]
        }
        '--upstream' {
            if ($Index + 1 -ge $args.Count) { Fail '--upstream needs a value' }
            $Index++
            $Upstream = $args[$Index]
        }
        default { Fail "unknown argument: $($args[$Index])" }
    }
}

Set-Location $RepoRoot
if (-not (Test-Path (Join-Path $Upstream 'trafilatura/utils.py'))) { Fail "$Upstream is not an upstream trafilatura checkout" }
$Upstream = (Resolve-Path $Upstream).Path
git rev-parse --verify --quiet "$Base^{commit}" *> $null
if ($LASTEXITCODE -ne 0) { Fail "$Base is not a commit" }

# The upstream table of Cargo.toml up to the next table header.
function Get-PinTable {
    param([Parameter(Mandatory = $true)][AllowEmptyString()][string[]]$Lines)
    $InTable = $false
    foreach ($Line in $Lines) {
        if ($Line.StartsWith('[')) { $InTable = $Line.TrimEnd() -eq '[package.metadata.upstream]' }
        if ($InTable) { $Line.TrimEnd() }
    }
}

# The fixtures live under the pinned release, which is the same on both sides unless head-only.
$HeadPin = @(Get-PinTable (Get-Content 'Cargo.toml'))
$Release = ''
foreach ($Line in $HeadPin) {
    if ($Line -match '^version = "(.*)"$') { $Release = $Matches[1]; break }
}
if (-not $Release) { Fail 'Cargo.toml has no version under [package.metadata.upstream]' }
$Fixtures = Join-Path $RepoRoot ".temp/fixtures/$Release"
if (-not (Test-Path (Join-Path $Fixtures 'index.json'))) { Fail "no fixtures at $Fixtures; run scripts/fixtures.ps1" }

$BaseSrc = Join-Path $RepoRoot '.temp/regression/base-src'
$RegressionDir = Join-Path $RepoRoot '.temp/regression'

function Remove-BaseWorktree {
    # A failed remove is not an error: prune drops a worktree whose directory is already gone.
    if (git worktree list --porcelain | Select-String -Quiet '^worktree .*/\.temp/regression/base-src$') {
        git worktree remove --force $BaseSrc *> $null
    }
}

# A run killed before its finally leaves the worktree registered.
Remove-BaseWorktree
Remove-Item -Recurse -Force $RegressionDir -ErrorAction SilentlyContinue
git worktree prune
$null = New-Item -ItemType Directory -Force "$RegressionDir/base", "$RegressionDir/head"

$HeadOnly = ''
git diff --quiet $Base -- tests/measure tests/measure_heap.rs tests/common benches
if ($LASTEXITCODE -ne 0) {
    $HeadOnly = 'measuring code differs between base and head'
} else {
    $BasePin = @(Get-PinTable (git show "${Base}:Cargo.toml"))
    if (($BasePin -join "`n") -ne ($HeadPin -join "`n")) { $HeadOnly = 'upstream pin differs between base and head' }
}

# Runs the heap sample and the bench in the tree at $Tree and leaves heap-sample.json and criterion/ in $Out.
# Both trees build into the head's target dir, so the dependencies compile once. Criterion writes
# <group>/<bench>/new/estimates.json under CRITERION_HOME instead of the target dir.
function Measure-Tree {
    param([Parameter(Mandatory = $true)][string]$Tree, [Parameter(Mandatory = $true)][string]$Out)
    Push-Location $Tree
    $env:CARGO_TARGET_DIR = Join-Path $RepoRoot 'target'
    $env:TRAFILATURA_UPSTREAM = $Upstream
    $env:TRAFILATURA_FIXTURES = $Fixtures
    try {
        [Console]::Error.WriteLine("== heap sample in $Tree ==")
        $env:TRAFILATURA_MEASURE = '1'
        $env:TRAFILATURA_MEASURE_ONLY = '@sample'
        cargo test --locked --release --features __internals --test measure_heap -- --nocapture 2>&1 | ForEach-Object { [Console]::Error.WriteLine($_) }
        if ($LASTEXITCODE -ne 0) { Fail "the heap sample failed in $Tree" }
        Remove-Item Env:TRAFILATURA_MEASURE, Env:TRAFILATURA_MEASURE_ONLY
        [Console]::Error.WriteLine("== bench in $Tree ==")
        $env:CRITERION_HOME = Join-Path $Out 'criterion'
        cargo bench --locked --features __internals --bench extract -- --noplot 2>&1 | ForEach-Object { [Console]::Error.WriteLine($_) }
        if ($LASTEXITCODE -ne 0) { Fail "the bench failed in $Tree" }
    } finally {
        Remove-Item Env:CARGO_TARGET_DIR, Env:TRAFILATURA_FIXTURES, Env:CRITERION_HOME, Env:TRAFILATURA_MEASURE, Env:TRAFILATURA_MEASURE_ONLY -ErrorAction SilentlyContinue
        Pop-Location
    }
    Copy-Item (Join-Path $Tree ".temp/measure/$Release-rust-heap-sample.json") (Join-Path $Out 'heap-sample.json')
}

$Code = 1
try {
    if (-not $HeadOnly) {
        git worktree add --detach $BaseSrc $Base *>&1 | ForEach-Object { [Console]::Error.WriteLine($_) }
        if ($LASTEXITCODE -ne 0) { Fail "git worktree add failed for $Base" }
        Measure-Tree $BaseSrc "$RegressionDir/base"
    }
    Measure-Tree $RepoRoot "$RegressionDir/head"

    $HeadOnlyArgs = if ($HeadOnly) { @('--head-only', $HeadOnly) } else { @() }
    $Summary = uv run --frozen --no-dev --directory tools/parity python -m parity.measure --compare $RegressionDir @HeadOnlyArgs
    $Code = $LASTEXITCODE
    $Summary
    if ($env:GITHUB_STEP_SUMMARY) { Add-Content -Path $env:GITHUB_STEP_SUMMARY -Value $Summary }
} finally {
    Remove-BaseWorktree
    git worktree prune
}
exit $Code
