<#
.SYNOPSIS
  Build the per-user rs-term MSI with the WiX CLI (`wix` must be on PATH).

.DESCRIPTION
  The version is read from crates/rs-term/Cargo.toml and reduced to x.y.z
  (the MSI ProductVersion must be numeric). Pass -Cert to ship the public
  code-signing certificate in the install folder (signed releases only).
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string] $Exe,
  [Parameter(Mandatory = $true)][string] $Out,
  [string] $Cert
)

$ErrorActionPreference = 'Stop'
$root = (Resolve-Path (Join-Path $PSScriptRoot '..\..')).Path

$line = Select-String -LiteralPath (Join-Path $root 'crates\rs-term\Cargo.toml') -Pattern '^version = "(\d+\.\d+\.\d+)' | Select-Object -First 1
if (-not $line) { throw 'Could not read a numeric version from crates/rs-term/Cargo.toml.' }
$version = $line.Matches[0].Groups[1].Value
Write-Host "MSI version: $version"

$defs = @('-d', "Version=$version", '-d', "ExePath=$((Resolve-Path -LiteralPath $Exe).Path)", '-d', "RepoRoot=$root")
if ($Cert) { $defs += @('-d', "CertPath=$((Resolve-Path -LiteralPath $Cert).Path)") }

wix build (Join-Path $root 'installer\rs-term.wxs') -arch x64 -o $Out @defs
if ($LASTEXITCODE -ne 0) { throw "wix build failed with exit code $LASTEXITCODE" }

# ICE91 only warns that the per-user files are not per-machine; that is intended.
wix msi validate -sice ICE91 $Out
if ($LASTEXITCODE -ne 0) { throw "MSI validation failed with exit code $LASTEXITCODE" }
