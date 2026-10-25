#Requires -Version 7.2

# Measure the parity corpus: time (5-run median) and heap per page. The Rust side covers decode, parse and
# extraction of every page plus the synthetic pages; the Python side decode + parse only. The Rust targets
# write .temp/measure/<version>-rust-time.json and <version>-rust-heap.json (and -synthetic.json files),
# the Python side <version>-python.json; --merge writes <version>-rust.json and the merged <version>.json and
# prints a Markdown table. Needs the fixtures (scripts/fixtures.ps1): their index.json
# lists the pages. Run it by hand; the numbers depend on the machine. The Rust targets take about 5 minutes
# and need about 1.5 GB of heap for the synthetic pages.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$ScriptName = 'measure.ps1'
# Same image and uv as scripts/fixtures.ps1
$DockerImage = 'python:3.13-slim@sha256:98ce6fb13b3803744fbca11434f00818f1e300e4e48a4f204e07b2c928b6218e'
$UvVersion = '0.11.14'
# Pages listed one per line in the table when --only is used; without it, totals only
$TableRows = 15

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [--upstream PATH] [--docker] [--only SUBSTRING]

Runs tests/measure_time.rs and tests/measure_heap.rs (release build, feature __internals; two binaries,
because the dhat allocator of the heap one would slow the timings), then tools/parity (parity.measure) against
the upstream checkout at PATH, then merges both into .temp/measure/<version>.json. The Python side runs
where scripts/fixtures.ps1 runs: uv on Linux, Docker elsewhere. On Windows and macOS the two sides
therefore run on different systems (host and container): compare ratios between runs, not absolutes.

  --upstream PATH  Upstream git checkout. Default: `$env:TRAFILATURA_UPSTREAM, else ../trafilatura.
  --docker         Run the Python side in Docker even on Linux.
  --only SUBSTRING Keep pages whose upstream path contains SUBSTRING.
  -h, --help       Show this message.
"@
}

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Upstream = if ($env:TRAFILATURA_UPSTREAM) { $env:TRAFILATURA_UPSTREAM } else { Join-Path $RepoRoot '../trafilatura' }
$UseDocker = $false
$Only = ''
for ($Index = 0; $Index -lt $args.Count; $Index++) {
    switch ($args[$Index]) {
        { $_ -in '-h', '--help' } { Show-Usage; exit 0 }
        '--upstream' {
            if ($Index + 1 -ge $args.Count) { Fail '--upstream needs a value' }
            $Index++
            $Upstream = $args[$Index]
        }
        '--docker' { $UseDocker = $true }
        '--only' {
            if ($Index + 1 -ge $args.Count) { Fail '--only needs a value' }
            $Index++
            $Only = $args[$Index]
        }
        default { Fail "unknown argument: $($args[$Index])" }
    }
}

Set-Location $RepoRoot
if (-not (Test-Path (Join-Path $Upstream 'trafilatura/utils.py'))) { Fail "$Upstream is not an upstream trafilatura checkout" }
$Upstream = (Resolve-Path $Upstream).Path
$null = New-Item -ItemType Directory -Force '.temp/measure'
$Temp = (Resolve-Path '.temp').Path
$OnlyArgs = if ($Only) { @('--only', $Only) } else { @() }
$Rows = if ($Only) { $TableRows } else { 0 }

[Console]::Error.WriteLine('== Rust: load_html over the corpus (time, then heap) ==')
$env:TRAFILATURA_UPSTREAM = $Upstream
$env:TRAFILATURA_MEASURE = '1'
$env:TRAFILATURA_MEASURE_ONLY = $Only
try {
    cargo test --release --features __internals --test measure_time --test measure_heap -- --nocapture 2>&1 | ForEach-Object { [Console]::Error.WriteLine($_) }
    if ($LASTEXITCODE -ne 0) { Fail 'the Rust measurement failed' }
} finally {
    Remove-Item Env:TRAFILATURA_MEASURE, Env:TRAFILATURA_MEASURE_ONLY -ErrorAction SilentlyContinue
}

[Console]::Error.WriteLine('== Python: upstream load_html ==')
if (-not $UseDocker -and $IsLinux) {
    uv run --frozen --no-dev --directory tools/parity python -m parity.measure --upstream $Upstream --out "$RepoRoot/.temp/measure" @OnlyArgs
    if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    uv run --frozen --no-dev --directory tools/parity python -m parity.measure --merge --out "$RepoRoot/.temp/measure" --rows $Rows
    exit $LASTEXITCODE
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { Fail 'docker is required on this platform (or pass --docker on Linux)' }
$Inner = 'rows=$1 && shift && pip install --quiet --root-user-action=ignore "uv==$0" && uv sync --frozen --no-dev --quiet && uv run --frozen --no-dev python -m parity.measure --upstream /upstream --out /work/.temp/measure "$@" && exec uv run --frozen --no-dev python -m parity.measure --merge --out /work/.temp/measure --rows "$rows"'
docker run --rm `
    -v "${RepoRoot}:/work:ro" `
    -v "${Upstream}:/upstream:ro" `
    -v "${Temp}:/work/.temp" `
    -e PYTHONDONTWRITEBYTECODE=1 -e TRAFILATURA_MEASURE_RUNTIME=docker -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy `
    -w /work/tools/parity $DockerImage `
    sh -c $Inner $UvVersion $Rows @OnlyArgs
exit $LASTEXITCODE
