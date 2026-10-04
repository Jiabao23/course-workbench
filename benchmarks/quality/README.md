# 参考稿与质量测量

先以音频 SHA256 固定样本，再由人工逐句回听、填写参考稿。`reviewed:false`
是草稿；填写审核人和日期、完成回听后才改为 `true`。不可直接把 ASR 输出标为
人工参考。无法听清的样本另列，开发集与保留验证集分开；目前尚无人工审核参考集。

参照 `reference.schema.json`。`text` 是全段参考文字，`terms` 是实际出现的术语；
`speech_intervals_ms` 是人工标注讲话范围。`omission_intervals_ms` 是针对这份识别
结果人工标注的漏转区间，不能拿一个引擎的漏转标注评判另一个引擎。
待评估结果中的同名区间字段表示系统发出的漏转告警。其准确/召回指标按时间长度
计算，不是按事件数量。讲话覆盖率只验证时间覆盖，不能证明这些时间里的字正确。

```powershell
python benchmarks/quality/evaluate.py --audio sample.wav --result done.json --reference reference.json --output report.json
python -m unittest discover -s benchmarks/quality/tests -v
```

不传 `--reference` 时只提取性能，质量状态为 `not_measured`。传入未审核参考稿、
音频摘要不一致、空参考文字时拒绝输出质量结论。计算规范化 CER 时采用 NFKC、
casefold、去空白/Unicode 标点；不转换繁简体。同时记录原始 CER、术语出现次数的
缺失/多出量（不是语境准确率）。无预测区间的 precision 是 null，不能记作 100%。
单元测试使用人工构造真值，仅证明计算正确，不能据此宣布实际模型更准确。
