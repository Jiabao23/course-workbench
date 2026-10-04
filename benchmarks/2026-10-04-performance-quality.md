# v0.6.0 性能与可靠性记录

日期：2026-10-04。本机 Windows 10、RTX 3050 Laptop 4GB、驱动 526.56。
这些是开发设备的测量，不是其他显卡或干净系统的验收。没有人工审核参考稿，
CER、术语准确率、漏转检测召回率均为 **未测量**。

## 完整课程 GPU 对照

音频《课程目标》，356.608 秒，16 kHz 单声道 PCM16 WAV。
SHA256：`460b9eb7ca742ed6613996e3618d0f4bffdfc91e91b7d4e9498d2ccac913c4b5`。
同源 multilingual small，中文、空提示、8 线程、greedy、best_of=1、temperature=0、
上下文继承、固定 300 秒块、batch=1、并发=1。原引擎有效 beam=None 与新引擎
beam=1 均为 greedy，但引擎实现、模型格式和输出不完全相同。

每配置三次，按原引擎 → Faster FP16 → Faster INT8 顺序交错。每次新进程、
重新加载模型、新检查点，文件系统缓存已热；计时期间无其他 ASR/VAD，桌面
后台仍在运行。进程时间包含启动、导入、哈希、加载、推理、检查点和退出，
不含网页下载/FFmpeg。全程记录，未只挑最快一轮。

| 配置 | 三次进程耗时（秒） | 中位数（秒） | 整卡显存采样峰值（MiB） | 进程 RAM 峰值范围（MiB） |
| --- | --- | --- | --- | --- |
| OpenAI Whisper / FP16 | 65.913 / 91.075 / 102.088 | 91.075 | 2777 | 3274–4000 |
| faster-whisper / FP16 | 36.737 / 56.530 / 61.935 | 56.530 | 1413 | 2016–2033 |
| faster-whisper / INT8_FLOAT16 | 48.584 / 49.300 / 51.458 | 49.300 | 1093 | 2028–2029 |

相对原引擎，本组中位数耗时分别减少约 38% 和 46%。FP16 单轮最好，但中位数
与 INT8 的次序不同；波动明显，未监控温度/频率，不将差异全部归因于算法。
不能由此保证所有课程加速，也不能据此把 INT8 改成默认质量档位。

显存由 `nvidia-smi` 每次查询后等待 200 ms 采样，是**整卡已用显存**（含背景），
不是每进程 allocator 峰值，可能漏过短峰值。原引擎还记录 Torch allocator；
新引擎不伪造该指标。表中只比较相同口径的整卡采样。RAM 为各进程峰值。

完整的配置、运行时、每阶段时间、模型摘要及各次结果见
[脱敏测量 JSON](experiments/2026-10-04-gpu-results.json)。原协议和完整转写留在
本机 `.local-data/performance-tests/benchmarks/*-final-*`，不提交课程全文或音频。
早期调试数据及与 VAD 重叠的 `faster-int8-final-1` 不纳入以上九次结果。

原引擎：Whisper 20250625 / Torch 2.4.1+cu118 / Python 3.12.7。
新引擎：faster-whisper 1.0.3 / CT2 3.24.0 / Python 3.11.15 / cuDNN 8.9.7 CUDA11。
新模型为 Systran/faster-whisper-small，revision
`536b0662742c02347bc0e980a01041f333bce120`；model.bin SHA256
`3e305921506d8872816023e4c273e75d2419fb89b24da97b4fe7bce14170d671`。
依赖快照见 [独立环境清单](experiments/2026-10-04-cu11-environment.txt)，仅记录本机。

## CPU 与真实中断恢复

- small / CPU INT8 / 8 线程：取同一音频前 60 秒，进程 21.043 秒，推理
  16.047 秒，17 个片段。这是一次可用性测量，不与全长 GPU 时间直接比较。
- small / CUDA FP16：70 秒、30 秒恢复块。首块落盘后强制终止真实 worker，
  重启复用 1 块，完成 26 个片段；已完成文件与片段 ID/文本未改写、无重复 ID。
  完全重放复用 3 块、未加载模型，分别耗时 8.969 / 1.687 秒。
  [恢复结果](experiments/2026-10-04-recovery.json)。
- 运行命令为 `scripts/verify-recovery.py --engine faster-whisper --compute-type float16`
  并提供音频、独立 Python、FFmpeg、模型根目录与一个新的 output 目录。

## 分块实验与模型常驻决定

`speech-boundary-v2-silent-context` 保留完整时间轴和词级归属校验。实际整段课程
与拼入静音的构造样本均因“不完整或无效词级时间戳”被明确拒绝，没有完成结果。
该失败暴露当前词级对齐的准入限制；单元测试证明规划/失败语义，不代表实际
边界识别质量通过。暂不开放默认入口，继续使用固定块。原始错误协议保存在
本机 `benchmarks/boundary-course`、`benchmarks/boundary-silence-fixture` 测试目录。

Faster FP16 模型加载约 1.2–1.6 秒，原引擎约 5.6–6.5 秒；相对完整课程推理，
常驻不是主要收益。连续短复核仍有可观的导入/加载成本，但本版选择保留
进程隔离和 4GB 设备的确定释放，暂不增加会话复用/压力驱逐协议。此决定
不是宣称短任务加载开销已消除。局部批量复核目前仍逐段启动 worker。

## 可复现入口

`benchmarks/experiments/run_worker.py --python <独立Python> --request <请求JSON> --output <结果目录>`。
请求使用合同中的 JSONL v1 字段，每次计时使用空的新检查点目录；resume 测量单列。
`benchmarks/quality/evaluate.py` 不传 reference 时仅提取性能；人工审核后再计算 CER。

区间算法的合成数据测量另见 `experiments/2026-10-04-interval.json`：包括排序、
合并和输出分配，排除 UI/JSON/指纹。新旧输出逐次相等，不能把内核加速倍数
当作整段转写的加速倍数。进程峰值包含两种算法及样本，不是各算法内存对照。

最终 release 区间内核测量：1,000 / 10,000 / 30,000 讲话区间的旧/新中位耗时分别为
0.232/0.031、21.878/0.342、206.604/0.751 ms（各五次，全部输出相等）。
整个基准进程峰值工作集 6,668,288 字节；不能据此比较两算法各自峰值内存。
