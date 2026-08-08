# Creates a git worktree with an isolated Zed data dir so multiple local builds
# can run side by side. Settings/keymaps stay shared with %APPDATA%\Zed via a
# junction; the database, window state, and extensions are per-worktree.
#
# Usage:
#   .\script\new-worktree.ps1 <name>            # new branch <name> from current HEAD
#   .\script\new-worktree.ps1 <name> <branch>   # check out an existing branch
param(
    [Parameter(Mandatory = $true)][string]$Name,
    [string]$Branch
)
$ErrorActionPreference = 'Stop'

$repoRoot = git rev-parse --show-toplevel
if ($LASTEXITCODE -ne 0) { throw "Not inside a git repository" }
$worktreePath = Join-Path (Split-Path $repoRoot -Parent) "zed-$Name"

if ($Branch) {
    git worktree add $worktreePath $Branch
} else {
    git worktree add -b $Name $worktreePath
}
if ($LASTEXITCODE -ne 0) { throw "git worktree add failed" }

$dataDir = Join-Path $env:LOCALAPPDATA "Zed-Local\$Name"
New-Item -ItemType Directory -Force $dataDir | Out-Null

$sharedConfig = Join-Path $env:APPDATA 'Zed'
New-Item -ItemType Directory -Force $sharedConfig | Out-Null
$configLink = Join-Path $dataDir 'config'
if (-not (Test-Path $configLink)) {
    New-Item -ItemType Junction -Path $configLink -Target $sharedConfig | Out-Null
}

Write-Host "Worktree: $worktreePath"
Write-Host "Data dir: $dataDir (config -> $sharedConfig)"
Write-Host ""
Write-Host "Run from the worktree:"
Write-Host "  cargo run --profile release-fast -- --user-data-dir `"$dataDir`""
