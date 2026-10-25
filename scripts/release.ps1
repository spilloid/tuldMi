<#
.SYNOPSIS
Builds the tuldMi Windows installer, optionally Authenticode-signed, and verifies it.

.EXAMPLE
./scripts/release.ps1 -Sign      # signed release build (needs `az login` + signer role)
./scripts/release.ps1            # unsigned local build
#>
param([switch]$Sign)
$ErrorActionPreference = 'Stop'
$repo = Split-Path $PSScriptRoot -Parent
Push-Location $repo
try {
    # Windows PowerShell 5.1 turns a native tool's stderr into terminating errors under 'Stop';
    # npm and the Tauri CLI log progress to stderr, so judge them by exit code only.
    $ErrorActionPreference = 'Continue'
    npm ci --no-audit --no-fund
    if ($LASTEXITCODE -ne 0) { throw 'npm ci failed' }

    $tauriArgs = @('tauri', 'build')
    if ($Sign) {
        # Tauri runs signCommand for the app executable (before it is packed into the
        # installer), the uninstaller, and the installer itself. %1 is the file to sign.
        $script = (Join-Path $PSScriptRoot 'sign-windows.ps1')
        $overlay = @{ bundle = @{ windows = @{ signCommand = @{
            cmd  = 'powershell'
            args = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', $script, '-Files', '%1')
        } } } } | ConvertTo-Json -Depth 6 -Compress
        $overlayPath = Join-Path ([IO.Path]::GetTempPath()) "tuldmi-sign-$([guid]::NewGuid()).json"
        [IO.File]::WriteAllText($overlayPath, $overlay, (New-Object System.Text.UTF8Encoding $false))
        $tauriArgs += @('--config', $overlayPath)
    }
    npx @tauriArgs
    if ($LASTEXITCODE -ne 0) { throw 'tauri build failed' }
    $ErrorActionPreference = 'Stop'

    $version = (Get-Content package.json -Raw | ConvertFrom-Json).version
    $installer = Join-Path $repo "target\release\bundle\nsis\tuldMi_${version}_x64-setup.exe"
    # Only the installer is checked: Tauri signs tuldmi.exe before packing it, then restores its
    # unpatched build output in target\release, so that copy always reads NotSigned. The signed
    # exe is the one inside the installer (verified by installing; see docs/RELEASE.md).
    foreach ($f in @($installer)) {
        $sig = Get-AuthenticodeSignature $f
        Write-Host ("{0}: {1} | {2} | timestamp: {3}" -f (Split-Path $f -Leaf), $sig.Status, $sig.SignerCertificate.Subject, [bool]$sig.TimeStamperCertificate)
        if ($Sign -and ($sig.Status -ne 'Valid' -or -not $sig.TimeStamperCertificate)) { throw "$f is not validly signed and timestamped" }
    }
    $hash = (Get-FileHash $installer -Algorithm SHA256).Hash.ToLower()
    [IO.File]::WriteAllText("$installer.sha256", "$hash  $(Split-Path $installer -Leaf)`n", (New-Object System.Text.UTF8Encoding $false))
    Write-Host "SHA-256 $hash"
} finally {
    if ($overlayPath) { Remove-Item $overlayPath -ErrorAction SilentlyContinue }
    Pop-Location
}
