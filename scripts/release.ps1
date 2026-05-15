#!/usr/bin/env pwsh
# Tag and push a release based on the version in Cargo.toml.
# Usage:  scripts\release.ps1            # tags v<Cargo.toml version> at HEAD and pushes
#         scripts\release.ps1 -DryRun    # show what would happen, don't tag/push
#         scripts\release.ps1 -Force     # skip clean-tree check
[CmdletBinding()]
param(
    [switch]$DryRun,
    [switch]$Force
)

$ErrorActionPreference = 'Stop'

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

$cargoToml = Join-Path $repoRoot 'Cargo.toml'
if (-not (Test-Path $cargoToml)) { throw "Cargo.toml not found at $cargoToml" }

$versionLine = Select-String -Path $cargoToml -Pattern '^\s*version\s*=\s*"([^"]+)"' | Select-Object -First 1
if (-not $versionLine) { throw 'Could not find version = "..." in Cargo.toml' }
$version = $versionLine.Matches[0].Groups[1].Value
$tag = "v$version"

Write-Output "Cargo.toml version: $version"
Write-Output "Tag to create:      $tag"

$status = & git status --porcelain
if ($status -and -not $Force) {
    Write-Output ''
    Write-Output 'Working tree is not clean:'
    Write-Output $status
    throw 'Refusing to tag a dirty tree. Commit/stash first, or rerun with -Force.'
}

$existingLocal = & git tag --list $tag
if ($existingLocal) { throw "Tag $tag already exists locally. Bump Cargo.toml version or delete the tag." }

$existingRemote = & git ls-remote --tags origin "refs/tags/$tag"
if ($existingRemote) { throw "Tag $tag already exists on origin. Bump Cargo.toml version." }

$head = (& git rev-parse --short HEAD).Trim()
Write-Output "HEAD commit:        $head"

if ($DryRun) {
    Write-Output ''
    Write-Output 'DRY RUN: would execute:'
    Write-Output "  git tag $tag $head"
    Write-Output "  git push origin $tag"
    return
}

& git tag $tag
if ($LASTEXITCODE -ne 0) { throw "git tag failed" }

& git push origin $tag
if ($LASTEXITCODE -ne 0) {
    & git tag -d $tag | Out-Null
    throw "git push failed; local tag removed so you can retry."
}

Write-Output ''
Write-Output "Pushed $tag. Release workflow should be running now."
Write-Output "Watch:  gh run list --workflow=release.yml --limit 3"
