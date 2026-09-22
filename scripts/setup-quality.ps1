param(
    [Parameter(Mandatory=$true)][string]$Python,
    [string]$Directory = 'D:\CourseWorkbenchRuntime\quality'
)
$ErrorActionPreference = 'Stop'
if (-not (Get-Command uv -ErrorAction SilentlyContinue)) { throw '请先安装 uv，或按 docs/setup.md 手动安装隔离依赖。' }
if (-not [IO.Path]::IsPathRooted($Directory)) { throw '扩展目录必须是绝对路径。' }
# Read the selected runtime without installing into or changing that environment.
$qualityTorchVersion = & $Python -c 'import importlib.metadata; import numpy; print(importlib.metadata.version("torch"))'
if ($LASTEXITCODE -ne 0) { throw '所选 Python 需要已有 torch 与 numpy，请先配置识别环境。' }
$qualityTorchVersion = $qualityTorchVersion.Trim()
$qualityBackend = if ($qualityTorchVersion.Contains('+')) { $qualityTorchVersion.Split('+')[1] } else { 'cpu' }
if ($qualityBackend -notmatch '^(cpu|cu\d+)$') { throw "此脚本未验证 $qualityBackend，请手动安装与 torch 配套的 torchaudio。" }
New-Item -ItemType Directory -Path $Directory -Force | Out-Null
uv pip install --python $Python --target $Directory --no-deps --link-mode copy 'silero-vad==6.2.2' 'onnxruntime==1.30.0' 'flatbuffers==25.12.19' 'protobuf==7.36.2' 'packaging==26.3' 'coloredlogs==15.0.1' 'humanfriendly==10.0'
if ($LASTEXITCODE -ne 0) { throw '扩展安装失败；原 Python 环境未修改。' }
uv pip install --python $Python --target $Directory --no-deps --link-mode copy "torchaudio==$qualityTorchVersion" --index-url "https://download.pytorch.org/whl/$qualityBackend"
if ($LASTEXITCODE -ne 0) { throw '配套 torchaudio 安装失败；请检查 torch 版本和网络。' }
Write-Output "已安装至 $Directory。请在工作台设置中的“语音检测扩展目录”填写此路径，并用一条课程运行检测。"
