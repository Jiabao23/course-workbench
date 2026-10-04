# v0.6.0 算法、性能与质量验证

日期：2026-10-04；基线 398475e / v0.5.0。测试数据在
`.local-data/performance-tests`，不写入用户资料库，不提交音频、全文、Cookie
或私有配置。独立 faster-whisper 环境未修改 bili2text 环境。

## 自动化检查

| 检查 | 本轮结果 |
| --- | --- |
| `cargo test --workspace --locked` | 132 通过，1 个进程测试辅助入口按设计单独 ignored |
| Worker `python -m unittest discover -s workers/asr/tests -v` | 52 通过 |
| 质量指标 `python -m unittest discover -s benchmarks/quality/tests -v` | 4 通过 |
| 前端 `npm test` | 41 通过 |
| Rust fmt / Clippy all-targets `-D warnings` | 通过 |
| TypeScript / Vite production build | 通过 |
| Windows Tauri NSIS 0.6.0 | 构建及静默安装成功 |

合计 229 项。日志保存在测试目录 `rust-tests.log`、`python-tests.log`、
`clippy.log`、`package.log`。首次 Rust 构建被测试应用占用 exe 阻止；结束
该隔离实例后重新完整运行并通过，没有把受阻构建计作通过。

覆盖区间算法与 5000 组随机旧算法对照、缓存身份及迁移、批量预算/交叠/
原子采纳/幂等、旧版本引用、来源证据提交失败回滚、损坏 manifest、配置指纹
随运行时变化、引擎加载/缺依赖、精度参数、检查点身份、实验边界歧义拒绝、
半毫秒舍入、候选差异及循环回听状态。构造数据和 mock 测试不记为硬件实测。

独立审查发现并修复：旧候选播放范围、来源证据提交时机、损坏 manifest 兼容
回退、原 Whisper 默认解码参数、Python/Rust 时长舍入、实验块边界丢字风险、
CT2 版本未进入验证指纹。最终只读复查未发现其他实质问题。

## 真实硬件和服务流程

《课程目标》356.608 秒全段：两引擎/三精度配置共九次 CUDA 测量；CPU INT8
另用前 60 秒验证。参数、波动和显存口径见 [性能记录](../benchmarks/2026-10-04-performance-quality.md)。
真实终止 worker 后复用首块并完成，完整重放不加载模型，ID/文本不重复。

最新 Rust Runtime 通过真实本地媒体导入 → FFmpeg → faster-whisper CUDA →
分块核验 → SQLite，保存 160 段、2/2 块、provenance 与 diagnostics。
核对报告明确提示该引擎诊断阈值未校准。不是通过 mock worker 代替该流程。

实验语音边界策略在整课及插入静音的构造音频上均因词级对齐不足而拒绝。
没有发布部分转写，也没有宣称实际边界质量通过；默认仍为固定块。
实际执行质量脚本：无参考稿输出 `not_measured`；未审核草稿退出码 2，拒绝
产生质量结果。没有 CER、专业术语准确率或实际漏转召回率结论。

## 真实桌面复核闭环

使用真实 WebView2 和本地音轨；人工构造两处缺漏，属于故障夹具，不是人工
逐字参考稿。故意移除 40–62 秒、300–330 秒附近片段，原版保留 135 段。

- 语音检测后显示 2 组 / 19 条疑点证据，另有 1 项分块依据缺失，没有隐藏证据。
- 实际 audio 元素循环播放观察到 5 次回绕，播放时间前进、无媒体错误。
- 批量预览扩展到 35.8–69.8 / 296.8–333.4 秒，合计 70.6 秒；真实 CUDA
  生成两个候选，第二候选按钮播放自己的范围，原疑点回听按钮暂停使用。
- 勾选两个候选一次采纳，只生成版本 2。旧版 135 段不变，旧 19 条证据标记
  已修订；新版本重新计算报告、复用同音频语音缓存，没有复制旧人工结论。
- 候选中仍有可见错词，因此 UI 不自动采纳，不以“无时间空白”宣布文字正确。
- 第二次独立取消夹具：首候选落盘后点击取消，约 110 ms 退出忙碌；数据库
  保留 1 候选、1 原版本，已完成候选继续显示，原文未修改。
  较早一次取消尝试晚于任务完成，不计作取消成功；上述是重新执行的有效记录。
- 截图保存在本机 `quality-v2.png`、`candidate-final.png`、`cancel-candidate.png`。

## 安装与数据保全

安装包：`output/release/Course-Workbench-0.6.0-windows-x64-setup.exe`。
SHA256：`0c13807c008ec9cff9261a486cbca25886b88b0dcce794583b1f8009966da1c7`。
NSIS 安装退出码 0，`D:\CourseWorkbench\course-workbench.exe` 文件/产品版本
均为 0.6.0。七个打包 Python 文件与源码 SHA256 一致。

安装前原应用未运行；已使用 SQLite backup 保存生产库、复制设置，逐表记录
内容摘要，并记录独立 Obsidian vault 的 15 个文件摘要。备份位于
`.local-data/performance-tests/production-backup`。

停止 Vite 后，安装版从 `http://tauri.localhost/` 读取内置前端。在隔离配置下
通过路径入口导入真实 60 秒 WAV，调用随包 faster adapter / CUDA FP16，保存
21 段、1/1 块及来源证据；时间戳回听达到 readyState=4、时间前进、无错误，
Markdown 实际写盘。该次使用应用默认 fallback 解码，不并入固定 temperature=0
的性能对照。独立环境、模型和音轨仍使用显式路径，不依赖开发服务器。

随后按正式配置启动，schema=6、integrity_check=ok。全部 19 个旧表逐行摘要一致，
包括 FTS、版本、笔记、引用、分类、任务和核对记录；原 3 份资料、3 个版本、
1250 个片段、1 条笔记、3 个任务均保留。Obsidian 15 文件摘要一致。设置仅新增
可选 fasterPythonPath / fasterModelDir，所有原键值一致，原引擎仍为默认。
备份目录 `before.json` / `after.json` 保留完整核对证据。

## 验证界限

未在第二台电脑、其他显卡或纯净 Windows 上实测；8GB/24GB 等仍属策略模拟。
无人工参考稿，不宣传质量提高。没有真实云 API 密钥，本轮未测云服务质量。
历史原生选择/跨窗口拖入的 Windows 自动化限制仍存在；本轮安装导入使用
路径入口。模型常驻和实验分块质量晋级均未启用，理由与证据已记录。

## GitHub 交付

功能提交 `ef6801b4b79b662e32a1295a859324faf0d2819f` 已推送至
`feat/course-workbench-v1`，已用远端 ref 核对 SHA。[Windows CI](https://github.com/Jiabao23/course-workbench/actions/runs/37213316196)
在交付核查时运行中；本地检查与安装验证已通过，未把排队/运行中状态写为远端通过。
