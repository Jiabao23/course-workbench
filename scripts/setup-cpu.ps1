param(
    [Parameter(Mandatory=$true)][string]$Directory,
    [Parameter(Mandatory=$true)][string]$Python
)
$ErrorActionPreference = 'Stop'
$courseRuntime = [IO.Path]::GetFullPath($Directory)
if (-not [IO.Path]::IsPathRooted($Directory) -or $courseRuntime -eq [IO.Path]::GetPathRoot($courseRuntime)) {
    throw 'Choose an absolute folder inside a drive, for example D:\CourseWorkbenchRuntime.'
}
if (-not (Test-Path -LiteralPath $Python -PathType Leaf)) { throw 'Select an existing Python 3.12 x64 executable.' }
$courseVenv = Join-Path $courseRuntime 'venv'
if (Test-Path -LiteralPath $courseVenv) { throw 'The target venv already exists. Select another folder to preserve it.' }
New-Item -ItemType Directory -Path $courseRuntime -Force | Out-Null
$env:PIP_CACHE_DIR = Join-Path $courseRuntime 'pip-cache'
& $Python -m venv $courseVenv
if ($LASTEXITCODE -ne 0) { throw 'Unable to create the isolated environment.' }
$coursePython = Join-Path $courseVenv 'Scripts\python.exe'
& $coursePython -m pip install --upgrade pip
if ($LASTEXITCODE -ne 0) { throw 'pip upgrade failed.' }
& $coursePython -m pip install 'torch>=2.4,<3' --index-url https://download.pytorch.org/whl/cpu
if ($LASTEXITCODE -ne 0) { throw 'CPU Torch installation failed. Existing application data is unchanged.' }
& $coursePython -m pip install 'openai-whisper==20250625' 'numpy>=1.26,<3' 'yt-dlp>=2025.1'
if ($LASTEXITCODE -ne 0) { throw 'Whisper dependencies could not be installed.' }
& $coursePython -c 'import torch, whisper, numpy, yt_dlp; print("torch:", torch.__version__); print("whisper:", whisper.__version__); print("CPU environment ready")'
if ($LASTEXITCODE -ne 0) { throw 'Dependency import verification failed.' }
Write-Output "Set the application's Python path to: $coursePython"
Write-Output 'Configure FFmpeg/ffprobe and a model directory in the app, then run a sample verification.'
