#Requires -Version 7.2

# Fuzz one target with libFuzzer, or minimize one crash artifact. Runs scripts/fuzz.sh in a rust
# container at the CI toolchain, so the host needs only Docker.

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

# PowerShell 7.4+ can turn a native command's stderr into a terminating error, and docker and cargo
# write their progress there.
if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$ScriptName = 'fuzz.ps1'
# Moves with rust-version and the toolchain pin of _ci.yml (OI-6 in docs/phase-plan.md).
$Image = 'rust:1.98.1'
$CargoFuzzVersion = '0.13.2'
$Targets = @('parse_document', 'extract')

function Show-Usage {
    @"
Usage: scripts/$ScriptName [-h] <target> [seconds]
       scripts/$ScriptName --tmin <target> <artifact>

Runs scripts/fuzz.sh with the same arguments in a $Image container: fuzzes <target> (parse_document or
extract) for [seconds] (a positive integer, default 60), or with --tmin minimizes <artifact>, a crash
file under the repository, with the same limits. Needs Docker; cargo-fuzz is installed into the
Docker volume trafilatura-fuzz-tools-stable on the first run. Run scripts/fuzz.sh -h for the libFuzzer flags.

Seed directories: tests/cache, tests/eval and tests/resources under `$env:TRAFILATURA_UPSTREAM, else
trafilatura beside the repository; mounted only if it exists.

  --tmin       minimize <artifact> instead of fuzzing.
  -h, --help   Show this message.
"@
}

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    Show-Usage | ForEach-Object { $Host.UI.WriteErrorLine($_) }
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

if ($args.Count -gt 0 -and $args[0] -in '-h', '--help') { Show-Usage; exit 0 }

$ScriptArgs = @($args)
if ($ScriptArgs.Count -gt 0 -and $ScriptArgs[0] -eq '--tmin') {
    if ($ScriptArgs.Count -ne 3) { Fail '--tmin takes <target> <artifact>' }
    if ($ScriptArgs[1] -notin $Targets) { Fail "unknown or missing target: $($ScriptArgs[1])" }
} else {
    if ($ScriptArgs.Count -lt 1 -or $ScriptArgs.Count -gt 2) { Fail 'expected <target> [seconds]' }
    if ($ScriptArgs[0] -notin $Targets) { Fail "unknown or missing target: $($ScriptArgs[0])" }
    if ($ScriptArgs.Count -eq 2 -and $ScriptArgs[1] -notmatch '^[1-9][0-9]*$') {
        Fail "seconds must be a positive integer: $($ScriptArgs[1])"
    }
}

$RepoRoot = Split-Path -Parent $PSScriptRoot

# An artifact is passed to the container as a path under /work.
if ($ScriptArgs[0] -eq '--tmin') {
    if (-not (Test-Path -PathType Leaf $ScriptArgs[2])) { Fail "artifact not found: $($ScriptArgs[2])" }
    $Artifact = (Resolve-Path $ScriptArgs[2]).ProviderPath
    $Root = (Resolve-Path $RepoRoot).ProviderPath
    if (-not $Artifact.StartsWith($Root + [IO.Path]::DirectorySeparatorChar)) {
        Fail "artifact must be under the repository: $Artifact"
    }
    $ScriptArgs[2] = $Artifact.Substring($Root.Length + 1).Replace('\', '/')
}

docker info *> $null
if ($LASTEXITCODE -ne 0) {
    $Host.UI.WriteErrorLine("$ScriptName`: Docker is not running")
    exit 1
}

$Upstream = if ($env:TRAFILATURA_UPSTREAM) { $env:TRAFILATURA_UPSTREAM } else { Join-Path (Split-Path -Parent $RepoRoot) 'trafilatura' }
$UpstreamMount = @()
if (Test-Path -PathType Container $Upstream) {
    $UpstreamMount = @('-v', "$((Resolve-Path $Upstream).ProviderPath):/upstream:ro", '-e', 'TRAFILATURA_UPSTREAM=/upstream')
}

# The container is Linux: its target dir, registry and tools live in named volumes, not in the repository.
$Command = 'export PATH=/opt/fuzz-tools/bin:$PATH; ' +
    'rustup target add x86_64-unknown-linux-gnuasan >/dev/null; ' +
    "command -v cargo-fuzz >/dev/null || cargo install --root /opt/fuzz-tools cargo-fuzz --version $CargoFuzzVersion --locked; " +
    'scripts/fuzz.sh "$@"'

docker run --rm `
    -v "$((Resolve-Path $RepoRoot).ProviderPath):/work" `
    -v trafilatura-fuzz-target:/work/fuzz/target `
    -v trafilatura-fuzz-cargo:/usr/local/cargo/registry `
    -v trafilatura-fuzz-tools-stable:/opt/fuzz-tools `
    @UpstreamMount `
    -w /work $Image `
    bash -c $Command fuzz.ps1 @ScriptArgs
exit $LASTEXITCODE
