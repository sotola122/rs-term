<#
.SYNOPSIS
  Authenticode-sign files with a PFX and verify the result.

.DESCRIPTION
  The PFX comes from -PfxBase64 / -PfxPassword (default: the environment
  variables WINDOWS_SIGN_PFX_BASE64 / WINDOWS_SIGN_PFX_PASSWORD). It is decoded
  to a temp file that is deleted in a finally block. -CertOut also writes the
  public certificate as DER (.cer). The timestamp URL defaults to DigiCert and
  can be overridden with -TimestampUrl or SIGN_TIMESTAMP_URL.

  A self-issued certificate is not trusted on the runner, so the check
  requires a signature made with the PFX certificate (same thumbprint), not
  Status=Valid. The status is printed.
#>
[CmdletBinding()]
param(
  [Parameter(Mandatory = $true, Position = 0)]
  [string[]] $Files,
  [string] $PfxBase64 = $env:WINDOWS_SIGN_PFX_BASE64,
  [string] $PfxPassword = $env:WINDOWS_SIGN_PFX_PASSWORD,
  [string] $TimestampUrl = $(if ($env:SIGN_TIMESTAMP_URL) { $env:SIGN_TIMESTAMP_URL } else { 'http://timestamp.digicert.com' }),
  [string] $CertOut
)

$ErrorActionPreference = 'Stop'

if ([string]::IsNullOrWhiteSpace($PfxBase64)) { throw 'The code-signing PFX (WINDOWS_SIGN_PFX_BASE64) is missing or empty.' }
if ([string]::IsNullOrEmpty($PfxPassword)) { throw 'The code-signing PFX password (WINDOWS_SIGN_PFX_PASSWORD) is missing or empty.' }
if ($env:GITHUB_ACTIONS -eq 'true') { Write-Host "::add-mask::$PfxPassword" }

foreach ($f in $Files) {
  if (-not (Test-Path -LiteralPath $f -PathType Leaf)) { throw "File to sign not found: $f" }
}

$signtool = Get-ChildItem -Path "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
  Sort-Object { [version]$_.Directory.Parent.Name } -Descending |
  Select-Object -First 1
if (-not $signtool) { throw 'signtool.exe was not found under Windows Kits\10\bin\*\x64.' }
Write-Host "signtool: $($signtool.FullName)"

$pfx = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName() + '.pfx')
try {
  [IO.File]::WriteAllBytes($pfx, [Convert]::FromBase64String($PfxBase64))
  $cert = [Security.Cryptography.X509Certificates.X509Certificate2]::new($pfx, $PfxPassword)
  Write-Host "certificate: $($cert.Subject) thumbprint $($cert.Thumbprint)"

  if ($CertOut) {
    [IO.File]::WriteAllBytes($CertOut, $cert.Export([Security.Cryptography.X509Certificates.X509ContentType]::Cert))
    Write-Host "public certificate written to $CertOut"
  }

  & $signtool.FullName sign /fd SHA256 /f $pfx /p $PfxPassword /tr $TimestampUrl /td SHA256 @Files
  if ($LASTEXITCODE -ne 0) { throw "signtool failed with exit code $LASTEXITCODE" }

  foreach ($f in $Files) {
    $sig = Get-AuthenticodeSignature -LiteralPath $f
    $thumb = if ($sig.SignerCertificate) { $sig.SignerCertificate.Thumbprint } else { '(none)' }
    Write-Host "$f : status=$($sig.Status) signer=$thumb"
    if ($thumb -ne $cert.Thumbprint) { throw "Signature check failed for ${f}: signer $thumb, expected $($cert.Thumbprint)" }
  }
}
finally {
  if (Test-Path -LiteralPath $pfx) { Remove-Item -LiteralPath $pfx -Force }
}
