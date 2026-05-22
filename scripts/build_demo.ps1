# =========================================================
#  build_demo.ps1
#  只构建 examples/wasapi_exclusive_demo,不需要 Qt / vcpkg
# =========================================================
[CmdletBinding()]
param(
    [ValidateSet('Win32','x64')]
    [string]$Arch = 'Win32',                       # 默认 32-bit

    [ValidateSet('Release','Debug','RelWithDebInfo')]
    [string]$Config = 'Release',

    [string]$BuildDir = 'build',

    [ValidateSet('wasapi_exclusive_demo','wasapi_callback_demo','wasapi_play_wav_demo','wasapi_devices_demo','cli_player_demo')]
    [string]$Target = 'cli_player_demo',           # 默认构建最完整的 demo(交互式 CLI)

    [Parameter(ValueFromRemainingArguments=$true)]
    [string[]]$RunArgs,                            # 传递给 demo 的参数(如 wav 路径)

    [switch]$Run                                   # 加 -Run 构建后立即运行
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Write-Host ""
Write-Host "AudioPlayerX86 — WASAPI demo build" -ForegroundColor Cyan
Write-Host "  Root    : $root"
Write-Host "  Arch    : $Arch"
Write-Host "  Config  : $Config"
Write-Host "  Target  : $Target"
Write-Host "  BuildDir: $BuildDir"
Write-Host ""

# 检测 cmake
$cmake = (Get-Command cmake -ErrorAction SilentlyContinue)
if (-not $cmake) {
    Write-Error "未找到 cmake,请先安装 CMake 3.20+ 并加入 PATH"
}

# 配置
Push-Location $root
try {
    & cmake -S . -B $BuildDir -A $Arch `
            -DAPX_BUILD_APP=OFF `
            -DAPX_BUILD_EXAMPLES=ON `
            -DAPX_BUILD_TESTS=OFF
    if ($LASTEXITCODE -ne 0) { throw "CMake 配置失败" }

    & cmake --build $BuildDir --config $Config --target $Target -j
    if ($LASTEXITCODE -ne 0) { throw "构建失败" }

    $exe = Join-Path $root "$BuildDir\bin\$Config\$Target.exe"
    if (-not (Test-Path $exe)) {
        # 某些生成器输出路径不同,兜底搜索
        $found = Get-ChildItem -Path (Join-Path $root $BuildDir) -Filter "$Target.exe" -Recurse -ErrorAction SilentlyContinue | Select-Object -First 1
        if ($found) { $exe = $found.FullName }
    }

    Write-Host ""
    Write-Host "构建成功: $exe" -ForegroundColor Green

    if ($Run) {
        Write-Host ""
        Write-Host "▶ 运行 $Target" -ForegroundColor Yellow
        if ($RunArgs -and $RunArgs.Count -gt 0) {
            & $exe @RunArgs
        } else {
            & $exe
        }
    } else {
        Write-Host "运行: $exe"
    }
}
finally {
    Pop-Location
}
