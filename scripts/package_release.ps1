# =========================================================
#  package_release.ps1
#  Package Release - Bundles executable + DLLs into a zip file, excluding demos/logs.
# =========================================================
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

$srcDir = Join-Path $root "build_app\bin\Release"
$distDir = Join-Path $root "build_app\dist"
$pkgName = "SeraphAudioPlayer-v0.1.0-Win64"
$tempDir = Join-Path $distDir $pkgName
$zipFile = Join-Path $distDir "$pkgName.zip"

Write-Host "=========================================" -ForegroundColor Cyan
Write-Host " Starting packaging for Seraph Audio Player v0.1.0 " -ForegroundColor Cyan
Write-Host "=========================================" -ForegroundColor Cyan

if (-not (Test-Path $srcDir)) {
    throw "Release directory not found. Please compile the project first: $srcDir"
}

# 1. Prepare directories
if (Test-Path $tempDir) { Remove-Item -Recurse -Force $tempDir }
if (Test-Path $zipFile) { Remove-Item -Force $zipFile }
New-Item -ItemType Directory -Force -Path $tempDir | Out-Null
New-Item -ItemType Directory -Force -Path $distDir | Out-Null

# 2. Copy main application and DLLs
Write-Host "Copying main application and dependent libraries..." -ForegroundColor Yellow
$files = Get-ChildItem -Path $srcDir -File
foreach ($file in $files) {
    $name = $file.Name
    # Exclude demo executables, log files, and debug database files
    if ($name -like "*_demo.exe" -or $name -eq "apx.log" -or $name.EndsWith(".pdb") -or $name.EndsWith(".ilk")) {
        continue
    }
    Copy-Item -Path $file.FullName -Destination $tempDir -Force
}

# 3. Copy subdirectories (Qt plugins, QML plugins, etc.)
Write-Host "Copying subdirectories..." -ForegroundColor Yellow
$dirs = Get-ChildItem -Path $srcDir -Directory
foreach ($dir in $dirs) {
    Copy-Item -Path $dir.FullName -Destination (Join-Path $tempDir $dir.Name) -Recurse -Force
}

# 4. Copy documentation
$readme = Join-Path $root "README.md"
if (Test-Path $readme) {
    Copy-Item -Path $readme -Destination (Join-Path $tempDir "README.txt") -Force
}

# 5. Compress to ZIP
Write-Host "Compressing to ZIP archive..." -ForegroundColor Yellow
Compress-Archive -Path "$tempDir\*" -DestinationPath $zipFile -Force

# 6. Clean up temporary directory
Remove-Item -Recurse -Force $tempDir

# 7. Get package size
$sizeMB = [Math]::Round(((Get-Item $zipFile).Length / 1MB), 2)

Write-Host ""
Write-Host "[OK] Packaging successful!" -ForegroundColor Green
Write-Host "  ZIP Package Path : $zipFile" -ForegroundColor Green
Write-Host "  Package Size     : $sizeMB MB" -ForegroundColor Green
Write-Host "=========================================" -ForegroundColor Cyan
