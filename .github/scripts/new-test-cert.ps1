<#
.SYNOPSIS
  Create an ephemeral self-signed code-signing certificate for CI self-tests.
  The PFX (base64) and a random password are appended to the given env file
  (GITHUB_ENV) as WINDOWS_SIGN_PFX_BASE64 / WINDOWS_SIGN_PFX_PASSWORD, both
  masked in the log. The certificate is removed from the store before exit.
#>
[CmdletBinding()]
param([Parameter(Mandatory = $true)][string] $EnvFile)

$ErrorActionPreference = 'Stop'
$password = [Convert]::ToBase64String([Security.Cryptography.RandomNumberGenerator]::GetBytes(24))
if ($env:GITHUB_ACTIONS -eq 'true') { Write-Host "::add-mask::$password" }
$cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject 'CN=rs-term CI self-test' -CertStoreLocation Cert:\CurrentUser\My
$pfx = Join-Path ([IO.Path]::GetTempPath()) ([IO.Path]::GetRandomFileName() + '.pfx')
try {
  Export-PfxCertificate -Cert $cert -FilePath $pfx -Password (ConvertTo-SecureString $password -AsPlainText -Force) | Out-Null
  $b64 = [Convert]::ToBase64String([IO.File]::ReadAllBytes($pfx))
}
finally {
  Remove-Item -LiteralPath $pfx -Force -ErrorAction SilentlyContinue
  Remove-Item -LiteralPath "Cert:\CurrentUser\My\$($cert.Thumbprint)" -Force
}
if ($env:GITHUB_ACTIONS -eq 'true') { Write-Host "::add-mask::$b64" }
Add-Content -LiteralPath $EnvFile -Value "WINDOWS_SIGN_PFX_BASE64=$b64"
Add-Content -LiteralPath $EnvFile -Value "WINDOWS_SIGN_PFX_PASSWORD=$password"
