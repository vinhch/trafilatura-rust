#Requires -Version 7.2

# Measure the parity corpus: time (5-run median) and heap per page. The Rust side covers decode, parse and
# extraction of every page plus the synthetic pages; the Python side times decode + parse and extraction.
# The Rust targets write .temp/measure/<version>-rust-time.json and <version>-rust-heap.json (and
# -synthetic.json files), the Python side <version>-python.json; --merge writes <version>-rust.json and the
# merged <version>.json and prints a Markdown table. Needs the fixtures (scripts/fixtures.ps1): their
# index.json lists the pages. Run it by hand; the numbers depend on the machine. The Rust targets take about
# 5 minutes and need about 1 GB of heap for the synthetic pages.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$ScriptName = 'measure.ps1'
$DockerImage = 'trafilatura-measure'
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
in the same place as the Rust side: natively on Linux, in one Linux container (scripts/measure.Dockerfile)
elsewhere, so both sides always share a system. Absolute numbers still depend on the machine.

  --upstream PATH  Upstream git checkout. Default: `$env:TRAFILATURA_UPSTREAM, else ../trafilatura.
  --docker         Run both sides in the container even on Linux. On a Linux host the container runs as root, so
                   it leaves the .temp/measure outputs owned by root.
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
$OnlyArgs = if ($Only) { @('--only', $Only) } else { @() }
$Rows = if ($Only) { $TableRows } else { 0 }

if ($UseDocker -or -not $IsLinux) {
    if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { Fail 'docker is required on this platform (or run on Linux)' }
    docker build -f (Join-Path $PSScriptRoot 'measure.Dockerfile') -t $DockerImage $PSScriptRoot 2>&1 | ForEach-Object { [Console]::Error.WriteLine($_) }
    if ($LASTEXITCODE -ne 0) { Fail "docker build of $DockerImage failed" }
    # The target dir, cargo registry and uv cache live in named volumes: they survive --rm and keep a Linux
    # target dir out of the host checkout
    docker run --rm `
        -v "${RepoRoot}:/work" `
        -v "${Upstream}:/upstream:ro" `
        -v trafilatura-measure-target:/work/target `
        -v trafilatura-measure-cargo:/usr/local/cargo/registry `
        -v trafilatura-measure-uv:/root/.cache/uv `
        -e TRAFILATURA_MEASURE_RUNTIME=docker -e TRAFILATURA_UPSTREAM=/upstream `
        -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy -e PYTHONDONTWRITEBYTECODE=1 `
        -w /work $DockerImage `
        scripts/measure.sh @OnlyArgs
    exit $LASTEXITCODE
}

[Console]::Error.WriteLine('== Rust: load_html and extract over the corpus (time, then heap) ==')
$env:TRAFILATURA_UPSTREAM = $Upstream
$env:TRAFILATURA_MEASURE = '1'
$env:TRAFILATURA_MEASURE_ONLY = $Only
try {
    cargo test --locked --release --features __internals --test measure_time --test measure_heap -- --nocapture 2>&1 | ForEach-Object { [Console]::Error.WriteLine($_) }
    if ($LASTEXITCODE -ne 0) { Fail 'the Rust measurement failed' }
} finally {
    Remove-Item Env:TRAFILATURA_MEASURE, Env:TRAFILATURA_MEASURE_ONLY -ErrorAction SilentlyContinue
}

[Console]::Error.WriteLine('== Python: upstream load_html and extract ==')
uv run --frozen --no-dev --directory tools/parity python -m parity.measure --upstream $Upstream --out "$RepoRoot/.temp/measure" @OnlyArgs
if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
uv run --frozen --no-dev --directory tools/parity python -m parity.measure --merge --out "$RepoRoot/.temp/measure" --rows $Rows
exit $LASTEXITCODE
