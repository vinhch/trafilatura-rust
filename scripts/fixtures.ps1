#Requires -Version 7.2

# Parity fixtures: upstream Python trafilatura at the pinned commit, run over its test corpus. They
# live in git as one archive, tools/parity/fixtures/fixtures-<version>.tar.zst, and are unpacked into
# .temp/fixtures/<version>/. Generating them (--generate) is slow and runs on Linux only (the lxml wheels
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
Usage: scripts/$ScriptName
       scripts/$ScriptName [--generate] [--pack] [--upstream PATH] [--docker] [--only SUBSTRING]

Without flags: unpacks tools/parity/fixtures/fixtures-<version>.tar.zst into .temp/fixtures/<version>/, or does
nothing when that directory exists (delete it to unpack again). Needs no Python or Docker, only a tar
that reads zstd: ``tar --zstd`` when zstd is on PATH, else bsdtar (on Windows
%SystemRoot%\System32\tar.exe). Otherwise install zstd.

  --generate       Run tools/parity (parity.generate) against the upstream checkout and write
                   .temp/fixtures/<version>/ (index.json, pages/*.json, f1.json). The checkout must be at the
                   commit pinned in Cargo.toml. Dependencies come from tools/parity/uv.lock.
  --pack           Write the archive of .temp/fixtures/<version>/ (parity.pack), after --generate when both are
                   given. An existing archive is kept: delete it to pack again. After a new upstream release or a
                   generator change delete the archive and .temp/fixtures/<version>, then run
                   --generate --pack.
  --upstream PATH  Upstream git checkout. Default: `$env:TRAFILATURA_UPSTREAM, else ../trafilatura.
  --docker         Run --generate and --pack in Docker even on Linux. Without it, Linux uses uv directly.
  --only SUBSTRING Keep pages whose upstream path contains SUBSTRING (skips f1.json); implies --generate.
  -h, --help       Show this message.
"@
}

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Upstream = if ($env:TRAFILATURA_UPSTREAM) { $env:TRAFILATURA_UPSTREAM } else { Join-Path $RepoRoot '../trafilatura' }
$UseDocker = $false
$Generate = $false
$Pack = $false
$Only = ''
for ($Index = 0; $Index -lt $args.Count; $Index++) {
    switch ($args[$Index]) {
        { $_ -in '-h', '--help' } { Show-Usage; exit 0 }
        '--generate' { $Generate = $true }
        '--pack' { $Pack = $true }
        '--upstream' {
            if ($Index + 1 -ge $args.Count) { Fail '--upstream needs a value' }
            $Index++
            $Upstream = $args[$Index]
            $Generate = $true
        }
        '--docker' { $UseDocker = $true }
        '--only' {
            if ($Index + 1 -ge $args.Count) { Fail '--only needs a value' }
            $Index++
            $Only = $args[$Index]
            $Generate = $true
        }
        default { Fail "unknown argument: $($args[$Index])" }
    }
}

Set-Location $RepoRoot
# Only the `version` key of [package.metadata.upstream]: the package's own `version` comes earlier
$InTable = $false
$Version = ''
foreach ($Line in Get-Content 'Cargo.toml') {
    if ($Line.StartsWith('[')) { $InTable = $Line.TrimEnd() -eq '[package.metadata.upstream]' }
    elseif ($InTable -and $Line -match '^version = "(.*)"\s*$') { $Version = $Matches[1]; break }
}
if (-not $Version) { Fail 'no [package.metadata.upstream] version in Cargo.toml' }
$Archive = "tools/parity/fixtures/fixtures-$Version.tar.zst"
$FixturesDir = '.temp/fixtures'

function Expand-Fixtures {
    if (Test-Path "$FixturesDir/$Version") {
        Write-Output "fixtures present, delete .temp/fixtures/$Version to unpack again"
        return
    }
    if (-not (Test-Path $Archive)) { Fail "no archive for version $Version at $Archive; run scripts/$ScriptName --generate --pack" }
    $Tar = $null
    if (Get-Command zstd -ErrorAction SilentlyContinue) {
        $Tar = @('tar', '--zstd')
    } elseif ((tar --version 2>&1) -match 'bsdtar') {
        $Tar = @('tar')
    } elseif ($env:SYSTEMROOT -and (Test-Path "$env:SYSTEMROOT/System32/tar.exe")) {
        $Tar = @("$env:SYSTEMROOT/System32/tar.exe")
    } else {
        Fail 'no tar that reads zstd: install zstd, or bsdtar'
    }
    # Extract beside the target and rename, so an interrupted run never leaves a half-filled directory
    $Staging = "$FixturesDir/$Version.tmp"
    if (Test-Path $Staging) { Remove-Item -Recurse -Force $Staging }
    $null = New-Item -ItemType Directory -Force $Staging
    & $Tar[0] @($Tar | Select-Object -Skip 1) -xf $Archive -C $Staging --strip-components=1
    if ($LASTEXITCODE -ne 0) { Fail "extracting $Archive failed" }
    Move-Item $Staging "$FixturesDir/$Version"
    Write-Output "unpacked $Archive into $FixturesDir/$Version"
}

if (-not $Generate -and -not $Pack) {
    Expand-Fixtures
    exit 0
}

if ($Pack -and $Only) { Fail '--pack needs a full set: it cannot be combined with --only' }
if ($Generate -and $Pack -and (Test-Path $Archive)) {
    Write-Output "archive exists, delete it to pack again: $Archive"
    exit 0
}
if ($Generate) {
    if (-not (Test-Path (Join-Path $Upstream 'trafilatura/utils.py'))) { Fail "$Upstream is not an upstream trafilatura checkout" }
    $Upstream = (Resolve-Path $Upstream).Path
}
$null = New-Item -ItemType Directory -Force $FixturesDir, 'tools/parity/fixtures'
$Temp = (Resolve-Path '.temp').Path
$ArchiveDir = (Resolve-Path 'tools/parity/fixtures').Path
$OnlyArgs = if ($Only) { @('--only', $Only) } else { @() }

if (-not $UseDocker -and $IsLinux) {
    if ($Generate) {
        uv run --frozen --no-dev --directory tools/parity python -m parity.generate --upstream $Upstream --out "$RepoRoot/$FixturesDir" @OnlyArgs
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    if ($Pack) {
        uv run --frozen --no-dev --directory tools/parity python -m parity.pack --fixtures "$RepoRoot/$FixturesDir" --out "$RepoRoot/tools/parity/fixtures"
        if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
    }
    exit 0
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { Fail 'docker is required on this platform (or pass --docker on Linux)' }
$Mounts = @('-v', "${RepoRoot}:/work:ro", '-v', "${Temp}:/work/.temp", '-v', "${ArchiveDir}:/work/tools/parity/fixtures")
$GenerateArgs = @()
if ($Generate) {
    $Mounts += @('-v', "${Upstream}:/upstream:ro")
    $GenerateArgs = @('--upstream', '/upstream', '--out', '/work/.temp/fixtures') + $OnlyArgs
}
# $0 = uv version, $1 and $2 = run generate and pack (1 or 0), the rest goes to parity.generate
$Inner = 'pip install --quiet --root-user-action=ignore "uv==$0" && uv sync --frozen --no-dev --quiet && gen=$1 && pack=$2 && shift 2 &&
  { [ "$gen" = 0 ] || uv run --frozen --no-dev python -m parity.generate "$@"; } &&
  { [ "$pack" = 0 ] || uv run --frozen --no-dev python -m parity.pack --fixtures /work/.temp/fixtures --out /work/tools/parity/fixtures; }'
$GenFlag = if ($Generate) { '1' } else { '0' }
$PackFlag = if ($Pack) { '1' } else { '0' }
docker run --rm @Mounts `
    -e PYTHONDONTWRITEBYTECODE=1 -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy `
    -w /work/tools/parity $DockerImage `
    sh -c $Inner $UvVersion $GenFlag $PackFlag @GenerateArgs
exit $LASTEXITCODE
