# 已确认设计

用户确认实施个人自用的本地课程知识工作台；Windows 优先，Rust + Tauri 2 + React/TypeScript；Python worker 复用 Whisper。完整批准方案的落地说明见：

- [产品范围](../../product.md)
- [实现架构](../../architecture.md)
- [资源策略](../../resource-policy.md)
- [桌面契约](../../../contracts/desktop-commands.md)
- [共享接口](../../../contracts/implementation-contract.md)

首版闭环：明确选择的课程导入 → 字幕优先/必要时音轨 ASR → 带时间戳校对 → 可追溯笔记 → 中文搜索/单课问答 → TXT/MD/SRT/VTT 导出。

资料和全文检索保存在本地。云端或本地兼容 API 只收到使用者明确选中的文字，不包含媒体和 Cookie。资源能力检测与实测分开；不同设备可覆盖候选设置，首版并发 1。现有 bili2text 用作对照，不复用其源码。

独立项目位于 D 盘，工具下载与环境配置已获授权。保留用户 GitHub 仓库原有 Apache-2.0 LICENSE，交付到独立功能分支，避免覆盖 main。
