# 审查用例已迁入正式单元测试；直接运行回归，不修改测试源码。
# 修复后预期退出码为 0；任一包失败则返回 1。
param([ValidateSet('all', 'seraph-audio', 'seraph-tauri', 'seraph-visualizer')][string]$Package = 'all')
$ErrorActionPreference = 'Stop'
$auditWorkspace = (Resolve-Path -LiteralPath (Join-Path $PSScriptRoot '..\..')).Path
$auditPackages = @('seraph-audio', 'seraph-tauri', 'seraph-visualizer')
$auditFailed = $false
Push-Location -LiteralPath $auditWorkspace
try {
    foreach ($auditPackage in $auditPackages) {
        if ($Package -ne 'all' -and $Package -ne $auditPackage) { continue }
        & cargo test --offline --locked -p $auditPackage --lib bug_audit
        if ($LASTEXITCODE -ne 0) { $auditFailed = $true }
    }
} finally {
    Pop-Location
}
if ($auditFailed) { exit 1 }
exit 0
