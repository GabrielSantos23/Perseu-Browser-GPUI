# Builds the release binary and the installer into dist\.
# Set PERSEU_SIGN_THUMBPRINT to the SHA-1 thumbprint of a code-signing
# certificate in the current user's store to sign the exe, the installer and
# the uninstaller (SHA-256, RFC 3161 timestamp).
$ErrorActionPreference = "Stop"
$root = Split-Path -Parent $PSScriptRoot
$iscc = Join-Path ${env:ProgramFiles(x86)} "Inno Setup 6\ISCC.exe"
if (-not (Test-Path $iscc)) { throw "Inno Setup 6 not found at $iscc" }
$thumbprint = $env:PERSEU_SIGN_THUMBPRINT
$timestamp = "http://timestamp.digicert.com"

function Find-SignTool {
    $kits = Join-Path ${env:ProgramFiles(x86)} "Windows Kits\10\bin"
    Get-ChildItem $kits -Recurse -Filter signtool.exe -ErrorAction SilentlyContinue |
        Where-Object { $_.FullName -match "\\x64\\" } |
        Sort-Object FullName -Descending |
        Select-Object -First 1 -ExpandProperty FullName
}

Push-Location $root
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }
    $isccArgs = @()
    if ($thumbprint) {
        $signtool = Find-SignTool
        if (-not $signtool) { throw "signtool.exe not found in the Windows SDK" }
        $sign = "`"$signtool`" sign /sha1 $thumbprint /fd SHA256 /tr $timestamp /td SHA256"
        & cmd /c "$sign `"target\release\perseu.exe`""
        if ($LASTEXITCODE -ne 0) { throw "signing the exe failed" }
        $isccArgs += "/Ssigntool=$sign `$f"
        $isccArgs += "/DSign"
    } else {
        Write-Warning "PERSEU_SIGN_THUMBPRINT is not set; building unsigned binaries."
    }
    & $iscc @isccArgs (Join-Path $PSScriptRoot "perseu.iss")
    if ($LASTEXITCODE -ne 0) { throw "ISCC failed" }
} finally {
    Pop-Location
}
