<#
.SYNOPSIS
Authenticode-signs files with Azure Artifact Signing (Spillers Technology, corporate-strategy D-0047).

.DESCRIPTION
Called by Tauri's bundle.windows.signCommand (see scripts/release.ps1) for the app executable
before it is packed, and for the NSIS installer. Credentials come from DefaultAzureCredential
(`az login` locally). The signing identity needs the "Artifact Signing Certificate Profile Signer"
role on the certificate profile.

Adapted from SpoolSmith's scripts/sign-windows.ps1 with two fixes found while setting tuldMi up:
the metadata JSON is written without a UTF-8 BOM (Windows PowerShell 5.1's `Set-Content -Encoding
utf8` adds one, and the dlib rejects it with an opaque SignerSign() failure), and the dlib search
accepts the client tools' current layout (no x64 subfolder).

Never drops the RFC 3161 timestamp: Artifact Signing certificates are valid for three days.
#>
param(
    [Parameter(Mandatory)][string[]]$Files,
    [string]$Endpoint = $(if ($env:TULDMI_SIGN_ENDPOINT) { $env:TULDMI_SIGN_ENDPOINT } else { 'https://eus.codesigning.azure.net' }),
    [string]$AccountName = $(if ($env:TULDMI_SIGN_ACCOUNT) { $env:TULDMI_SIGN_ACCOUNT } else { 'jdspille' }),
    [string]$ProfileName = $(if ($env:TULDMI_SIGN_PROFILE) { $env:TULDMI_SIGN_PROFILE } else { 'primary-profile' }),
    [string]$SignTool = $env:TULDMI_SIGNTOOL,
    [string]$Dlib = $env:TULDMI_SIGN_DLIB
)
$ErrorActionPreference = 'Stop'

if ($Endpoint -notmatch '^https://[a-z0-9]+\.codesigning\.azure\.net/?$') {
    throw "Endpoint '$Endpoint' is not a regional Artifact Signing endpoint (e.g. https://eus.codesigning.azure.net)."
}

# DefaultAzureCredential shells out to `az`; make sure a freshly installed CLI is reachable.
$azBin = 'C:\Program Files\Microsoft SDKs\Azure\CLI2\wbin'
if (-not (Get-Command az -ErrorAction SilentlyContinue) -and (Test-Path $azBin)) {
    $env:PATH = "$azBin;$env:PATH"
}

if (-not $SignTool) {
    $SignTool = Get-ChildItem "${env:ProgramFiles(x86)}\Windows Kits\10\bin\*\x64\signtool.exe" -ErrorAction SilentlyContinue |
        Sort-Object { [version]($_.Directory.Parent.Name) } -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $SignTool -or -not (Test-Path $SignTool)) {
    throw 'signtool.exe not found. Install the Windows SDK (10.0.22621 or newer) or set TULDMI_SIGNTOOL.'
}
if (-not $Dlib) {
    $roots = @("$env:LOCALAPPDATA\Microsoft\MicrosoftArtifactSigningClientTools", $env:ProgramFiles, ${env:ProgramFiles(x86)}) |
        Where-Object { $_ -and (Test-Path $_) }
    $Dlib = Get-ChildItem -Path $roots -Filter 'Azure.CodeSigning.Dlib.dll' -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -notmatch '\\x86\\' } |
        Select-Object -First 1 -ExpandProperty FullName
}
if (-not $Dlib -or -not (Test-Path $Dlib)) {
    throw 'Azure.CodeSigning.Dlib.dll not found. Run: winget install -e --id Microsoft.Azure.ArtifactSigningClientTools'
}

$metadataDir = Join-Path ([IO.Path]::GetTempPath()) ([guid]::NewGuid())
New-Item -ItemType Directory -Force $metadataDir | Out-Null
$metadata = Join-Path $metadataDir 'metadata.json'
try {
    $json = @{
        Endpoint               = $Endpoint.TrimEnd('/')
        CodeSigningAccountName = $AccountName
        CertificateProfileName = $ProfileName
        CorrelationId          = "tuldmi-$(git rev-parse --short HEAD 2>$null)"
    } | ConvertTo-Json
    [IO.File]::WriteAllText($metadata, $json, (New-Object System.Text.UTF8Encoding $false))

    foreach ($file in $Files) {
        $path = (Resolve-Path $file).Path
        Write-Host "Signing $path"
        & $SignTool sign /fd SHA256 /tr 'http://timestamp.acs.microsoft.com' /td SHA256 /dlib $Dlib /dmdf $metadata $path
        if ($LASTEXITCODE -ne 0) { throw "signtool failed for $path (exit $LASTEXITCODE)" }
        $sig = Get-AuthenticodeSignature $path
        if ($sig.Status -ne 'Valid' -or -not $sig.TimeStamperCertificate) {
            throw "Signature on $path is $($sig.Status) / timestamped: $([bool]$sig.TimeStamperCertificate)"
        }
    }
} finally {
    Remove-Item $metadataDir -Recurse -Force -ErrorAction SilentlyContinue
}
