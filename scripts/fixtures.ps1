#Requires -Version 7.2

# Generate the parity fixtures: upstream Python trafilatura at the pinned commit, run over its test
# corpus, written to .temp/fixtures/<pin>/. Fixtures are generated on Linux only (the lxml wheels
# bundle libxml2 2.14.6 there, 2.11.9 on Windows), so Windows and macOS use Docker.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$ScriptName = 'fixtures.ps1'
# python:3.13-slim
$DockerImage = 'python:3.13-slim@sha256:98ce6fb13b3803744fbca11434f00818f1e300e4e48a4f204e07b2c928b6218e'
$UvVersion = '0.11.14'

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName [--upstream PATH] [--docker] [--only SUBSTRING]

Runs tools/parity (parity.generate) against the upstream checkout at PATH and writes
.temp/fixtures/<pin>/ (index.json, pages/*.json, f1.json). The checkout must be at the commit
pinned in Cargo.toml. Dependencies come from tools/parity/uv.lock.

  --upstream PATH  Upstream git checkout. Default: `$env:TRAFILATURA_UPSTREAM, else ../trafilatura.
  --docker         Run in Docker even on Linux. Without it, Linux uses uv directly.
  --only SUBSTRING Keep pages whose upstream path contains SUBSTRING (skips f1.json).
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
$null = New-Item -ItemType Directory -Force '.temp/fixtures'
$Temp = (Resolve-Path '.temp').Path
$OnlyArgs = if ($Only) { @('--only', $Only) } else { @() }

if (-not $UseDocker -and $IsLinux) {
    uv run --frozen --no-dev --project tools/parity python -m parity.generate --upstream $Upstream --out "$RepoRoot/.temp/fixtures" @OnlyArgs
    exit $LASTEXITCODE
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { Fail 'docker is required on this platform (or pass --docker on Linux)' }
$Inner = 'pip install --quiet --root-user-action=ignore "uv==$0" && uv sync --frozen --no-dev --quiet && exec uv run --frozen --no-dev python -m parity.generate "$@"'
docker run --rm `
    -v "${RepoRoot}:/work:ro" `
    -v "${Upstream}:/upstream:ro" `
    -v "${Temp}:/work/.temp" `
    -e PYTHONDONTWRITEBYTECODE=1 -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy `
    -w /work/tools/parity $DockerImage `
    sh -c $Inner $UvVersion --upstream /upstream --out /work/.temp/fixtures @OnlyArgs
exit $LASTEXITCODE
