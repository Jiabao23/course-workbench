# Windows 安装与环境

## 桌面运行

安装包面向 Windows 10/11 x64，按当前用户安装。系统需 WebView2 Runtime；Tauri 安装程序会在缺少时引导安装。桌面功能不依赖 Rust/Node 开发环境。字幕导入、校对、中文检索、手写笔记和导出也不依赖 Python、模型或 FFmpeg。

首次打开到“设置 → 资料与工具”选择数据路径。默认优先 D 盘；没有 D 盘时使用用户文档目录。设置文件位于 `%LOCALAPPDATA%\CourseWorkbench\settings.json`，调试版使用仓库 `.local-data/settings.json`。环境变量 `COURSE_WORKBENCH_CONFIG` 仅用于开发/隔离验证，不是日常启动要求。

发布包包含 `workers/asr/worker.py`，不要单独移走 EXE 而丢弃资源目录。解压便携包时保留整个目录结构。

`cw-diagnostic.exe` 是可选诊断程序，正常使用不需要它。运行诊断命令前先退出使用同一资料库的桌面应用；资料库占用锁会拒绝重复打开，避免影响进行中的任务。

## 复用本机环境

在设置中选择已有的 Python 可执行文件、Whisper 模型目录、FFmpeg 和 ffprobe。yt-dlp 可以选择独立可执行文件，也可留空并使用所选 Python 的 `yt_dlp` 模块。

本项目开发机验证了 Python 3.12.7、Whisper 20250625、NumPy 1.26.4 和 Torch 2.4.1+cu118；RTX 3050 Laptop 4 GB / 驱动 526.56 可用 small 模型。其他设备应重新检测并小样本验证。

## 新建 CPU 环境

先从 [Python 官网](https://www.python.org/downloads/windows/)安装 Python 3.12 x64。从源码包执行：

```powershell
powershell -File .\scripts\setup-cpu.ps1 -Directory D:\CourseWorkbenchRuntime -Python C:\Path\To\python.exe
```

脚本只在指定目录创建虚拟环境，使用 [PyTorch 官方 CPU 索引](https://download.pytorch.org/whl/cpu)安装 CPU Torch，再装 Whisper 和 yt-dlp；不修改系统 Python 或现有 bili2text 环境。脚本会输出应填入应用的 Python 路径。下载依赖可能占用数 GB，建议 D 盘。

FFmpeg/ffprobe 从 [FFmpeg 官网 Windows 构建入口](https://ffmpeg.org/download.html#build-windows)获取，解压到独立工具目录，在应用内选择两个 EXE。仅音轨下载不等于下载完整视频；转写与回听会产生 PCM WAV 缓存，需预留磁盘空间。

## NVIDIA CUDA

在独立虚拟环境中按 [官方选择器](https://pytorch.org/get-started/locally/)选择 Torch/CUDA。先检查驱动与该 CUDA 运行时的兼容性，再使用应用“重新检测”和“60 秒样本验证”。若不可用，先选 CPU 或调整驱动/环境；不根据 GPU 型号直接认定可用。

## 常见问题

| 现象 | 操作 |
| --- | --- |
| 未找到识别环境 | 选择安装了 torch、whisper、numpy 的 Python，然后重新检测 |
| 模型未下载/校验失败 | 选择已有模型目录或点击下载，下载后校验 SHA256 |
| FFmpeg/ffprobe 不可用 | 选择实际 EXE，确认路径仍存在 |
| 显存不足 | 保留检查点，选较小模型/CPU，再按当前配置重试 |
| 字幕需要登录 | 配置本人的 Cookie，或明确选择音轨转写 |
| API 超时/401/429 | 检查模型名、地址、密钥、额度；已有资料仍可使用 |
| 凭据存储不可用 | 本地功能可继续；修复 Windows 凭据存储后再配置云端密钥 |

日志位于资料目录 `logs/`。请求协助时先移除 Cookie、密钥、私有链接及课程原文；不要把整个资料库提交到 GitHub。
