# =========================================================
#  build_app.ps1
#  构建主程序 AudioPlayerX86.exe,自动调用 windeployqt 部署 Qt DLLs
# =========================================================
[CmdletBinding()]
param(
    # Qt 6.5+ 安装路径(必须含 lib\cmake\Qt6),如 C:\Qt\6.5.3\msvc2019_64
    [string]$QtPath = $env:APX_QT_PATH,

    # vcpkg 根目录(可选)。设置后会自动注入 toolchain,让 opusfile 等可选依赖
    # 直接被 find_package 找到。
    # 优先级:参数 > $env:VCPKG_ROOT > 自动嗅探
    [string]$VcpkgRoot = $env:VCPKG_ROOT,

    [ValidateSet('Win32','x64')]
    [string]$Arch = 'x64',                  # Qt 6 官方仅提供 x64 二进制

    [ValidateSet('Release','Debug','RelWithDebInfo')]
    [string]$Config = 'Release',

    [string]$BuildDir = 'build_app',

    [switch]$WithExamples,    # 默认不构建 examples,加 -WithExamples 才编

    [switch]$Run              # 构建成功后直接启动 EXE
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

# ---------- 1. 解析 Qt 路径 ----------
if (-not $QtPath) {
    # 按 Arch 选 64/32 套件优先级
    $kit = if ($Arch -eq 'x64') { 'msvc2019_64' } else { 'msvc2019_32' }
    $candidates = @(
        "C:\Qt\6.7.0\$kit",
        "C:\Qt\6.6.3\$kit",
        "C:\Qt\6.6.0\$kit",
        "C:\Qt\6.5.3\$kit",
        "C:\Qt\6.5.0\$kit"
    )
    foreach ($c in $candidates) {
        if (Test-Path "$c\lib\cmake\Qt6") { $QtPath = $c; break }
    }
}

if (-not $QtPath -or -not (Test-Path "$QtPath\lib\cmake\Qt6")) {
    Write-Host "[FAIL] 找不到 Qt 6 安装。" -ForegroundColor Red
    Write-Host "请用以下任意方式指定:"
    Write-Host "  1. 参数:     .\scripts\build_app.ps1 -QtPath ""C:\Qt\6.5.3\msvc2019_32"""
    Write-Host "  2. 环境变量: `$env:APX_QT_PATH = ""C:\Qt\6.5.3\msvc2019_32"""
    Write-Host ""
    Write-Host "需要 Qt 6.5+ 的 msvc2019_32 x86 套件,"
    Write-Host "路径下必须存在 lib\cmake\Qt6 子目录。"
    exit 1
}

# ---------- 1b. 解析 vcpkg 工具链(可选) ----------
$VcpkgToolchain = $null
if (-not $VcpkgRoot) {
    # 嗅探常见路径
    $candidatesVcpkg = @(
        "C:\vcpkg",
        "C:\dev\vcpkg",
        "$env:USERPROFILE\vcpkg",
        "$env:USERPROFILE\source\repos\vcpkg"
    )
    foreach ($c in $candidatesVcpkg) {
        if (Test-Path "$c\scripts\buildsystems\vcpkg.cmake") { $VcpkgRoot = $c; break }
    }
}
if ($VcpkgRoot) {
    $tc = Join-Path $VcpkgRoot 'scripts\buildsystems\vcpkg.cmake'
    if (Test-Path $tc) {
        $VcpkgToolchain = $tc
    } else {
        Write-Warning "VCPKG_ROOT='$VcpkgRoot' 不含 scripts\buildsystems\vcpkg.cmake,忽略"
    }
}

Write-Host ""
Write-Host "AudioPlayerX86 — build main application" -ForegroundColor Cyan
Write-Host "  Root     : $root"
Write-Host "  Arch     : $Arch"
Write-Host "  Config   : $Config"
Write-Host "  QtPath   : $QtPath"
Write-Host "  BuildDir : $BuildDir"
Write-Host "  Examples : $(if ($WithExamples) {'ON'} else {'OFF'})"
if ($VcpkgToolchain) {
    Write-Host "  vcpkg    : $VcpkgRoot"
} else {
    Write-Host "  vcpkg    : (none — set `$env:VCPKG_ROOT or -VcpkgRoot to enable opusfile etc.)"
}
Write-Host ""

# ---------- 2. CMake 配置 ----------
$examplesFlag = if ($WithExamples) { 'ON' } else { 'OFF' }

Push-Location $root
try {
    $cmakeArgs = @(
        '-S', '.', '-B', $BuildDir, '-A', $Arch,
        "-DCMAKE_PREFIX_PATH=$QtPath",
        '-DAPX_BUILD_UI=ON',
        '-DAPX_BUILD_APP=ON',
        "-DAPX_BUILD_EXAMPLES=$examplesFlag",
        '-DAPX_BUILD_TESTS=OFF'
    )
    if ($VcpkgToolchain) {
        $cmakeArgs += "-DCMAKE_TOOLCHAIN_FILE=$VcpkgToolchain"
        # 让 vcpkg 选 triplet 与 Arch 匹配
        $triplet = if ($Arch -eq 'x64') { 'x64-windows' } else { 'x86-windows' }
        $cmakeArgs += "-DVCPKG_TARGET_TRIPLET=$triplet"
    }
    & cmake @cmakeArgs
    if ($LASTEXITCODE -ne 0) { throw "CMake configuration failed" }

    # ---------- 3. 构建 ----------
    & cmake --build $BuildDir --config $Config --target AudioPlayerX86 -j
    if ($LASTEXITCODE -ne 0) { throw "Build failed" }

    # ---------- 4. 定位产物 ----------
    $exe = Join-Path $root "$BuildDir\bin\$Config\SeraphAudioPlayer.exe"
    if (-not (Test-Path $exe)) {
        $found = Get-ChildItem -Path (Join-Path $root $BuildDir) -Filter 'SeraphAudioPlayer.exe' -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $exe = $found.FullName }
    }
    if (-not (Test-Path $exe)) { throw "Cannot find SeraphAudioPlayer.exe" }

    # ---------- 5. windeployqt ----------
    $windeployqt = Join-Path $QtPath "bin\windeployqt.exe"
    if (Test-Path $windeployqt) {
        Write-Host ""
        Write-Host "Running windeployqt..." -ForegroundColor Yellow
        & $windeployqt --release --no-translations --no-system-d3d-compiler `
                       --no-opengl-sw --qmldir "$root\ui\qml" $exe
        if ($LASTEXITCODE -ne 0) { Write-Warning "windeployqt failed" }
    } else {
        Write-Warning "Cannot find windeployqt.exe"
    }

    # ---------- 6. Finish ----------
    Write-Host ""
    Write-Host "[OK] Build Success" -ForegroundColor Green
    Write-Host "  EXE      : $exe"
    Write-Host "  Dir      : $(Split-Path -Parent $exe)"
    Write-Host "  Run      : Double click EXE to run"

    if ($Run) {
        Write-Host ""
        Write-Host "[RUN] Starting SeraphAudioPlayer.exe" -ForegroundColor Yellow
        Start-Process -FilePath $exe
    }
}
finally {
    Pop-Location
}
