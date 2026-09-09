$ErrorActionPreference = 'Stop'
$courseRoot = Split-Path -Parent $PSScriptRoot
$courseTools = Join-Path $courseRoot '.tools'
$env:RUSTUP_HOME = Join-Path $courseTools 'rustup'
$env:CARGO_HOME = Join-Path $courseTools 'cargo'
$env:RUSTUP_TOOLCHAIN = 'stable-x86_64-pc-windows-msvc'
$env:CARGO_BUILD_JOBS = '4'
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:CARGO_HOME\bin;$env:PATH"
$courseVs = Join-Path $courseTools 'vs-buildtools'
$courseDevModule = Join-Path $courseVs 'Common7\Tools\Microsoft.VisualStudio.DevShell.dll'
if (Test-Path -LiteralPath $courseDevModule) {
    Import-Module $courseDevModule
    Enter-VsDevShell -VsInstallPath $courseVs -SkipAutomaticLocation -DevCmdArguments '-arch=x64 -host_arch=x64' | Out-Null
}
