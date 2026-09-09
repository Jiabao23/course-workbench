param([Parameter(ValueFromRemainingArguments=$true)][string[]]$UiArguments)
$ErrorActionPreference = 'Stop'
$courseRoot = Split-Path -Parent $PSScriptRoot
Push-Location $courseRoot
try {
    $courseOutput = & npx.cmd --yes --package '@playwright/cli' playwright-cli '-s=course-ui' @UiArguments 2>&1
    $courseOutput | Write-Output
    if ($LASTEXITCODE -ne 0) { throw "Playwright CLI exit $LASTEXITCODE" }
    $snapshotLine = $courseOutput | Select-String -Pattern '\[Snapshot\]\(([^)]+)\)' | Select-Object -Last 1
    if ($snapshotLine) {
        $relative = $snapshotLine.Matches[0].Groups[1].Value
        $resolved = [System.IO.Path]::GetFullPath((Join-Path $courseRoot $relative))
        $allowed = [System.IO.Path]::GetFullPath((Join-Path $courseRoot '.playwright-cli')) + [System.IO.Path]::DirectorySeparatorChar
        if ($resolved.StartsWith($allowed,[System.StringComparison]::OrdinalIgnoreCase)) { Get-Content -LiteralPath $resolved }
    }
} finally { Pop-Location }
