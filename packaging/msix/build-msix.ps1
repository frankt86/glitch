#Requires -Version 5.1
<#
.SYNOPSIS
    Package Glitch into a signed MSIX file.

.DESCRIPTION
    Pipeline:
      1. Read version from workspace Cargo.toml
      2. Optionally build the release binary (skipped with -SkipBuild)
      3. Locate makeappx.exe / signtool.exe in the Windows 10/11 SDK
      4. Stage the package layout under target\msix-staging\
      5. Generate placeholder icon PNGs via System.Drawing
      6. Pack with makeappx.exe
      7. Sign: uses $env:MSIX_CERT_PFX + $env:MSIX_CERT_PASSWORD if set,
         otherwise generates a self-signed CN=GlitchDev cert for local dev

.USAGE
    # Full local build (from repo root):
    .\packaging\msix\build-msix.ps1

    # Skip cargo build (binary already at target\release\glitch.exe):
    .\packaging\msix\build-msix.ps1 -SkipBuild

    # Install the resulting MSIX locally (run as Administrator):
    Add-AppxPackage target\glitch.msix

    # To trust the dev cert first (run as Administrator):
    $sig = Get-AuthenticodeSignature target\glitch.msix
    $cert = $sig.SignerCertificate
    Import-Certificate -FilePath ([IO.Path]::GetTempFileName() + '.cer') `
        -CertStoreLocation Cert:\LocalMachine\TrustedPeople
    # -- or simply enable Developer Mode in Windows Settings.
#>
param(
    [switch]$SkipBuild
)

$ErrorActionPreference = 'Stop'

# Run from the repository root regardless of where the script is invoked from.
$repoRoot = Split-Path (Split-Path $PSScriptRoot -Parent) -Parent
Set-Location $repoRoot

# ── 1. Version ────────────────────────────────────────────────────────────────
# Version lives in the workspace Cargo.toml under [workspace.package].
$workspaceToml = Get-Content "Cargo.toml" -Raw
if ($workspaceToml -match '(?m)^\s*version\s*=\s*"([^"]+)"') {
    $semver = $Matches[1]
} else {
    $semver = "0.1.0"
    Write-Warning "Could not parse version from Cargo.toml; defaulting to $semver"
}
# MSIX version must be Major.Minor.Build.Revision (4 parts); pad with 0.
$parts = $semver.Split('.')
while ($parts.Count -lt 4) { $parts += '0' }
$msixVersion = ($parts[0..3] -join '.')
Write-Host "Packaging Glitch $semver (MSIX $msixVersion)" -ForegroundColor Cyan

# ── 2. Build binary ───────────────────────────────────────────────────────────
if (-not $SkipBuild) {
    Write-Host "Building release binary..." -ForegroundColor Cyan
    cargo build --release -p glitch
    if ($LASTEXITCODE -ne 0) { throw "cargo build --release failed (exit $LASTEXITCODE)" }
}

$binaryPath = "target\release\glitch.exe"
if (-not (Test-Path $binaryPath)) {
    throw "Binary not found at $binaryPath. Run without -SkipBuild or build manually first."
}

# ── 3. Locate SDK tools ───────────────────────────────────────────────────────
function Find-SdkTool([string]$Name) {
    $sdkBase = "C:\Program Files (x86)\Windows Kits\10\bin"
    if (-not (Test-Path $sdkBase)) { return $null }
    Get-ChildItem $sdkBase -Filter $Name -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match '\\x64\\' } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}

$makeappx = Find-SdkTool "makeappx.exe"
$signtool  = Find-SdkTool "signtool.exe"

if (-not $makeappx) { throw "makeappx.exe not found. Install Windows SDK: https://aka.ms/windowssdk" }
if (-not $signtool)  { throw "signtool.exe not found. Install Windows SDK: https://aka.ms/windowssdk" }
Write-Host "  makeappx: $makeappx"
Write-Host "  signtool:  $signtool"

# ── 4. Stage layout ───────────────────────────────────────────────────────────
$staging = "target\msix-staging"
Remove-Item $staging -Recurse -Force -ErrorAction SilentlyContinue
New-Item "$staging\Assets" -ItemType Directory | Out-Null

Copy-Item $binaryPath "$staging\glitch.exe"

# Substitute __VERSION__ in the manifest template.
$manifest = Get-Content "packaging\msix\AppxManifest.xml" -Raw
$manifest  = $manifest.Replace("__VERSION__", $msixVersion)
$manifest | Set-Content "$staging\AppxManifest.xml" -Encoding UTF8

# ── 5. Generate icon PNGs ─────────────────────────────────────────────────────
Write-Host "Generating icon assets..." -ForegroundColor Cyan
Add-Type -AssemblyName System.Drawing

# Colour palette matching the app's dark theme.
$colBg     = [System.Drawing.Color]::FromArgb(255, 15, 17, 23)       # #0f1117
$colAccent = [System.Drawing.Color]::FromArgb(255, 122, 162, 247)    # #7aa2f7

function New-GlitchIcon([string]$Path, [int]$W, [int]$H) {
    $bmp = [System.Drawing.Bitmap]::new($W, $H)
    $g   = [System.Drawing.Graphics]::FromImage($bmp)
    $g.SmoothingMode      = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
    $g.TextRenderingHint  = [System.Drawing.Text.TextRenderingHint]::AntiAliasGridFit

    # Background fill
    $bgBrush = [System.Drawing.SolidBrush]::new($colBg)
    $g.FillRectangle($bgBrush, 0, 0, $W, $H)
    $bgBrush.Dispose()

    # Accent border (inset 8 % on each side)
    $m    = [int]($W * 0.08)
    $penW = [float][Math]::Max(1.0, $W / 24.0)
    $pen  = [System.Drawing.Pen]::new($colAccent, $penW)
    $g.DrawRectangle($pen, $m, $m, $W - 2 * $m, $H - 2 * $m)
    $pen.Dispose()

    # Centred "G" glyph
    $fontSize = [float]($H * 0.50)
    $font  = [System.Drawing.Font]::new("Segoe UI", $fontSize, [System.Drawing.FontStyle]::Bold, [System.Drawing.GraphicsUnit]::Pixel)
    $brush = [System.Drawing.SolidBrush]::new($colAccent)
    $sf    = [System.Drawing.StringFormat]::new()
    $sf.Alignment     = [System.Drawing.StringAlignment]::Center
    $sf.LineAlignment = [System.Drawing.StringAlignment]::Center
    $rect  = [System.Drawing.RectangleF]::new(0, 0, $W, $H)
    $g.DrawString("G", $font, $brush, $rect, $sf)

    $font.Dispose(); $brush.Dispose(); $sf.Dispose(); $g.Dispose()
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $bmp.Dispose()
}

New-GlitchIcon "$staging\Assets\Square44x44Logo.png"     44  44
New-GlitchIcon "$staging\Assets\Square150x150Logo.png"  150 150
New-GlitchIcon "$staging\Assets\Wide310x150Logo.png"    310 150
New-GlitchIcon "$staging\Assets\StoreLogo.png"           50  50
New-GlitchIcon "$staging\Assets\SplashScreen.png"       620 300

# ── 6. Pack MSIX ──────────────────────────────────────────────────────────────
$msixOut = "target\glitch.msix"
Remove-Item $msixOut -Force -ErrorAction SilentlyContinue
Write-Host "Packing MSIX..." -ForegroundColor Cyan
# /nv  — skip semantic validation (icon-size warnings don't fail the build)
# /o   — overwrite output if present
& $makeappx pack /d $staging /p $msixOut /nv /o
if ($LASTEXITCODE -ne 0) { throw "makeappx pack failed (exit $LASTEXITCODE)" }

# ── 7. Sign ───────────────────────────────────────────────────────────────────
# Publisher in AppxManifest.xml is "CN=GlitchDev" — the signing cert's Subject
# must match this string exactly (including capitalisation and no extra spaces).
if ($env:MSIX_CERT_PFX) {
    # CI path: cert stored as a base64-encoded GitHub secret.
    # MSIX_CERT_PFX  = base64(pfx file bytes)
    # MSIX_CERT_PASSWORD = pfx password (may be empty string)
    Write-Host "Signing with provided certificate..." -ForegroundColor Cyan
    $certBytes = [Convert]::FromBase64String($env:MSIX_CERT_PFX)
    $certPath  = [IO.Path]::Combine($env:TEMP, "glitch-sign.pfx")
    [IO.File]::WriteAllBytes($certPath, $certBytes)
    try {
        & $signtool sign /fd sha256 /p "$env:MSIX_CERT_PASSWORD" /f $certPath $msixOut
        if ($LASTEXITCODE -ne 0) { throw "signtool sign failed (exit $LASTEXITCODE)" }
    } finally {
        Remove-Item $certPath -Force -ErrorAction SilentlyContinue
    }
} else {
    # Local dev path: generate a transient self-signed cert.
    Write-Host "Generating self-signed certificate (CN=GlitchDev) for dev signing..." -ForegroundColor Cyan
    $cert = New-SelfSignedCertificate `
        -Type          CodeSigningCert `
        -Subject       "CN=GlitchDev" `
        -KeyUsage      DigitalSignature `
        -FriendlyName  "Glitch Dev Signing" `
        -CertStoreLocation "Cert:\CurrentUser\My" `
        -NotAfter      (Get-Date).AddYears(2)

    $pfxPath = [IO.Path]::Combine($env:TEMP, "glitch-dev-sign.pfx")
    $pfxPwd  = ConvertTo-SecureString "GlitchDevCert!" -Force -AsPlainText
    Export-PfxCertificate -Cert $cert -FilePath $pfxPath -Password $pfxPwd | Out-Null
    try {
        & $signtool sign /fd sha256 /p "GlitchDevCert!" /f $pfxPath $msixOut
        if ($LASTEXITCODE -ne 0) { throw "signtool sign failed (exit $LASTEXITCODE)" }
    } finally {
        Remove-Item $pfxPath -Force -ErrorAction SilentlyContinue
    }

    Write-Host ""
    Write-Host "Signed with self-signed cert (CN=GlitchDev)." -ForegroundColor Yellow
    Write-Host "To install the MSIX, either:" -ForegroundColor Yellow
    Write-Host "  1. Enable Developer Mode (Settings > System > For developers)" -ForegroundColor Yellow
    Write-Host "  2. Or trust the cert first (run as Administrator):" -ForegroundColor Yellow
    Write-Host "       `$thumb = (Get-AuthenticodeSignature target\glitch.msix).SignerCertificate.Thumbprint" -ForegroundColor Yellow
    Write-Host "       Move-Item Cert:\CurrentUser\My\`$thumb Cert:\LocalMachine\TrustedPeople" -ForegroundColor Yellow
}

# ── Done ──────────────────────────────────────────────────────────────────────
$sizeMB = [Math]::Round((Get-Item $msixOut).Length / 1MB, 1)
Write-Host ""
Write-Host "Done: $msixOut  ($sizeMB MB)" -ForegroundColor Green
Write-Host "Install: Add-AppxPackage $msixOut" -ForegroundColor Green
