#Requires -Version 7.2

# Oracle: the value upstream Python trafilatura (at the pinned commit) returns for a snippet, for
# writing expected values of ported tests. Runs on Linux through uv, elsewhere in Docker, because the
# lxml wheels bundle libxml2 2.14.6 on Linux and 2.11.9 on Windows (see scripts/fixtures.ps1).

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

if (Test-Path Variable:PSNativeCommandUseErrorActionPreference) {
    $PSNativeCommandUseErrorActionPreference = $false
}

$ScriptName = 'oracle.ps1'
# Same image and uv as scripts/fixtures.ps1, the source of truth
$DockerImage = 'python:3.13-slim@sha256:98ce6fb13b3803744fbca11434f00818f1e300e4e48a4f204e07b2c928b6218e'
$UvVersion = '0.11.14'
# Checks the pin the way parity.generate does, then puts upstream on sys.path; the snippet follows it
$Prelude = @'
import os, pathlib
from parity import import_path
from parity.generate import check_pin
_root = pathlib.Path(os.environ["TRAFILATURA_UPSTREAM"])
check_pin(_root)
import_path(_root)
'@

function Fail {
    param([Parameter(Mandatory = $true)][string]$Message)
    $Host.UI.WriteErrorLine("$ScriptName`: $Message")
    exit 1
}

function Show-Usage {
    @"
Usage: scripts/$ScriptName PYTHON_CODE

Runs PYTHON_CODE with ``python -c`` in tools/parity with upstream trafilatura importable, and prints its
stdout. The checkout must be at the commit pinned in Cargo.toml. Dependencies come from
tools/parity/uv.lock. Linux runs uv directly; Windows and macOS run Docker.

  PYTHON_CODE  e.g. 'from trafilatura.utils import trim; print(repr(trim(" a  b ")))'

Environment: TRAFILATURA_UPSTREAM is the upstream checkout. Default: ../trafilatura.
"@
}

if ($args.Count -eq 1 -and $args[0] -in '-h', '--help') { Show-Usage; exit 0 }
if ($args.Count -ne 1) { Fail 'expected one argument, the Python code (see --help)' }

$RepoRoot = Split-Path -Parent $PSScriptRoot
$Upstream = if ($env:TRAFILATURA_UPSTREAM) { $env:TRAFILATURA_UPSTREAM } else { Join-Path $RepoRoot '../trafilatura' }
if (-not (Test-Path (Join-Path $Upstream 'trafilatura/utils.py'))) { Fail "$Upstream is not an upstream trafilatura checkout" }
$Upstream = (Resolve-Path $Upstream).Path
# Through the environment, so quotes and newlines reach Python unchanged
$env:ORACLE_CODE = $Prelude.TrimEnd() + "`n" + $args[0]

Set-Location $RepoRoot
if ($IsLinux) {
    $env:TRAFILATURA_UPSTREAM = $Upstream
    uv run --frozen --no-dev --directory tools/parity python -c $env:ORACLE_CODE
    exit $LASTEXITCODE
}

if (-not (Get-Command docker -ErrorAction SilentlyContinue)) { Fail 'docker is required on this platform' }
docker run --rm `
    -v "${RepoRoot}:/work:ro" `
    -v "${Upstream}:/upstream:ro" `
    -e PYTHONDONTWRITEBYTECODE=1 -e PIP_DISABLE_PIP_VERSION_CHECK=1 -e UV_PROJECT_ENVIRONMENT=/tmp/venv -e UV_LINK_MODE=copy `
    -e TRAFILATURA_UPSTREAM=/upstream -e ORACLE_CODE `
    -w /work/tools/parity $DockerImage `
    sh -c 'pip install --quiet --root-user-action=ignore "uv==$0" && uv sync --frozen --no-dev --quiet && exec uv run --frozen --no-dev python -c "$ORACLE_CODE"' $UvVersion
exit $LASTEXITCODE
