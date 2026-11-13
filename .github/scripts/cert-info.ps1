<#
.SYNOPSIS
  Write the identifying details of a public certificate (.cer) to a text file.

.DESCRIPTION
  The SHA-1 and SHA-256 values are hashes of the DER file, so users can check
  a downloaded .cer with: certutil -hashfile rs-term-codesign.cer SHA256
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true)][string] $Cert,
  [Parameter(Mandatory = $true)][string] $Out
)

$ErrorActionPreference = 'Stop'
$path = (Resolve-Path -LiteralPath $Cert).Path
$c = [Security.Cryptography.X509Certificates.X509Certificate2]::new($path)
$sha256 = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash
$sha1 = (Get-FileHash -LiteralPath $path -Algorithm SHA1).Hash

$lines = @(
  "File:       $(Split-Path -Leaf $path)",
  "Subject:    $($c.Subject)",
  "Issuer:     $($c.Issuer)",
  "NotBefore:  $($c.NotBefore.ToUniversalTime().ToString('yyyy-MM-dd HH:mm:ss')) UTC",
  "NotAfter:   $($c.NotAfter.ToUniversalTime().ToString('yyyy-MM-dd HH:mm:ss')) UTC",
  "SHA-256:    $sha256",
  "SHA-1:      $sha1",
  '',
  'Check a downloaded file with:',
  '  certutil -hashfile rs-term-codesign.cer SHA256',
  '  certutil -hashfile rs-term-codesign.cer SHA1'
)
Set-Content -LiteralPath $Out -Value $lines -Encoding utf8
$lines | Write-Host
