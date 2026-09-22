//! Explicit, cancellable audio checks. No downloads or silent transcript replacement.
use super::*;
use crate::integrity::{self, AudioCheck, IntegrityReport, Resolution};
use course_core::quality::RecheckCandidate;
use sha2::{Digest, Sha256};
use std::{io::Read, time::Duration};

struct QualityGuard<'a> {
    runtime: &'a Runtime,
    auxiliary: AuxiliaryGuard<'a>,
}
impl Drop for QualityGuard<'_> {
    fn drop(&mut self) {
        self.runtime.quality_control.lock().unwrap().take();
    }
}
struct TempAudio(PathBuf);
impl Drop for TempAudio {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
        let _ = fs::remove_file(self.0.with_extension("partial.wav"));
    }
}

fn audio_hash(path: &Path, control: &ProcessControl) -> Result<String> {
    let mut stream = fs::File::open(path).context("本地音频不可用，请先获取回听音频")?;
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        control.check()?;
        let n = stream.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
    }
    Ok(hex::encode(digest.finalize()))
}
fn audio_path(asset: &Asset) -> Result<PathBuf> {
    let path = PathBuf::from(
        asset
            .audio_path
            .as_deref()
            .context("缺少本地音频，请先获取回听音频；不会自动下载")?,
    );
    ensure!(path.is_file(), "回听音频已不存在，请重新获取");
    Ok(path)
}

impl Runtime {
    fn begin_quality(&self) -> Result<QualityGuard<'_>> {
        let auxiliary = self.begin_auxiliary()?;
        *self.quality_control.lock().unwrap() = Some(auxiliary.control.clone());
        Ok(QualityGuard {
            runtime: self,
            auxiliary,
        })
    }
    pub fn cancel_quality(&self) {
        if let Some(control) = self.quality_control.lock().unwrap().as_ref() {
            control.cancel();
        }
    }
    pub(super) fn enrich_quality(
        &self,
        asset: &Asset,
        transcript: &Transcript,
        report: &mut IntegrityReport,
    ) -> Result<()> {
        let db = self.db();
        let saved = db.quality_evidence(&transcript.id, "speech")?;
        let diagnostics = db.quality_evidence(&transcript.id, "diagnostics")?;
        let current_hash = audio_path(asset)
            .ok()
            .and_then(|p| audio_hash(&p, &ProcessControl::default()).ok());
        let identity = if saved.is_some() && current_hash.is_some() {
            self.detector_identity().ok()
        } else {
            None
        };
        let valid = saved.as_ref().filter(|v| {
            current_hash.as_deref() == v["audio_fingerprint"].as_str()
                && current_hash.is_some()
                && identity.as_deref() == v["detector_identity"].as_str()
                && identity.is_some()
        });
        integrity::add_quality_evidence(report, transcript, valid, diagnostics.as_ref())?;
        if current_hash.is_none() {
            report.audio_check = AudioCheck {
                status: "unavailable".into(),
                message: "没有可用本地音频，仅检查时间轴；可先获取回听音频".into(),
            };
        } else if saved.is_some() && valid.is_none() {
            report.audio_check = AudioCheck {
                status: "notRun".into(),
                message: "音频或检测环境已变化/不可用，需重新进行语音检测".into(),
            };
        }
        report.fingerprint = hex::encode(Sha256::digest(serde_json::to_vec(&(
            &report.fingerprint,
            &current_hash,
            "quality-v1",
        ))?));
        for review in db.issue_reviews(&transcript.id, &report.fingerprint)? {
            if let Some(issue) = report.issues.iter_mut().find(|i| i.id == review.issue_id) {
                issue.resolution = Some(Resolution {
                    status: review.status,
                    note: review.note,
                    reviewed_at: review.reviewed_at,
                });
            }
        }
        report.pending_count = report
            .issues
            .iter()
            .filter(|i| {
                i.severity == "warning"
                    && i.resolution.as_ref().is_none_or(|r| r.status == "pending")
            })
            .count();
        Ok(())
    }
    pub fn review_integrity_issue(
        &self,
        asset_id: &str,
        transcript_id: &str,
        fingerprint: &str,
        issue_id: &str,
        status: &str,
        note: &str,
    ) -> Result<IntegrityReport> {
        let _gate = self.mutation_gate.lock().unwrap();
        ensure!(
            ["pending", "confirmed"].contains(&status),
            "修订状态只能由采纳新版本产生"
        );
        let report = self.check_integrity(asset_id, transcript_id)?;
        ensure!(
            report.fingerprint == fingerprint,
            "检查依据已变化，请重新核对"
        );
        ensure!(
            report.issues.iter().any(|i| i.id == issue_id),
            "疑点不属于当前报告"
        );
        self.db()
            .save_issue_review(transcript_id, fingerprint, issue_id, status, note)?;
        self.check_integrity(asset_id, transcript_id)
    }
    pub fn detect_speech(&self, asset_id: &str, transcript_id: &str) -> Result<IntegrityReport> {
        let guard = self.begin_quality()?;
        let control = &guard.auxiliary.control;
        let db = self.db();
        let asset = db.get_asset(asset_id)?;
        let t = db.get_transcript(transcript_id)?;
        ensure!(t.asset_id == asset_id, "文字版本不属于该课程");
        let path = audio_path(&asset)?;
        let before = audio_hash(&path, control)?;
        let settings = self.settings();
        let wav = TempAudio(
            settings
                .cache_path("temp")
                .join(format!("vad-{}.wav", new_id())),
        );
        media::convert(&settings, &path, &wav.0, control, None, |_| Ok(()))?;
        let worker = WhisperWorker {
            path: self.worker_path.with_file_name("quality_worker.py"),
        };
        let mut result = worker
            .execute(
                &settings,
                json!({"command":"detect_speech","job_id":guard.auxiliary.id,"audio_path":wav.0}),
                control,
                &mut |_| Ok(()),
            )
            .context("语音检测失败；请检查 Python 和语音检测扩展目录，基础时间轴检查仍可用")?;
        ensure!(
            audio_hash(&path, control)? == before,
            "检测期间音频已变化，请重试"
        );
        result = json!({"speech":result["speech"],"audio_fingerprint":before,"detector_identity":result["detector_identity"],"detector_version":result["detector_version"]});
        let mut report = integrity::check(&asset, &t, None, None);
        integrity::add_quality_evidence(&mut report, &t, Some(&result), None)?;
        let _gate = self.mutation_gate.lock().unwrap();
        control.check()?;
        ensure!(
            db.get_asset(asset_id)?.audio_path == asset.audio_path,
            "音频来源已变化，请重试"
        );
        db.save_quality_evidence(transcript_id, "speech", &result)?;
        self.check_integrity(asset_id, transcript_id)
    }
    pub fn recheck_interval(
        &self,
        asset_id: &str,
        transcript_id: &str,
        mut start_ms: u64,
        mut end_ms: u64,
    ) -> Result<RecheckCandidate> {
        ensure!(
            end_ms > start_ms && end_ms - start_ms <= 120000,
            "局部复核请选择不超过 120 秒的范围"
        );
        let guard = self.begin_quality()?;
        let control = &guard.auxiliary.control;
        let db = self.db();
        let asset = db.get_asset(asset_id)?;
        let base = db.get_transcript(transcript_id)?;
        ensure!(
            base.asset_id == asset_id && asset.active_version_id.as_deref() == Some(transcript_id),
            "请在当前文字版本上复核"
        );
        let path = audio_path(&asset)?;
        let fingerprint = audio_hash(&path, control)?;
        // Never delete the unexamined half of a boundary segment.
        loop {
            let before = (start_ms, end_ms);
            for s in &base.segments {
                if s.start_ms < end_ms && s.end_ms > start_ms {
                    start_ms = start_ms.min(s.start_ms);
                    end_ms = end_ms.max(s.end_ms);
                }
            }
            if before == (start_ms, end_ms) {
                break;
            }
        }
        ensure!(
            end_ms - start_ms <= 120000 && end_ms <= asset.duration_ms,
            "包含完整边界片段后超出范围，请选择更短的区间"
        );
        let settings = self.resolved_settings_with_control(control)?;
        let device = settings.device.as_str();
        let id = new_id();
        let wav = TempAudio(
            settings
                .cache_path("temp")
                .join(format!("recheck-{id}.wav")),
        );
        let mut command = super::super::process::command(&settings.ffmpeg_path)?;
        command
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y", "-i"])
            .arg(&path)
            .args([
                "-ss",
                &format!("{:.3}", start_ms as f64 / 1000.0),
                "-t",
                &format!("{:.3}", (end_ms - start_ms) as f64 / 1000.0),
                "-vn",
                "-ac",
                "1",
                "-ar",
                "16000",
                "-c:a",
                "pcm_s16le",
            ])
            .arg(&wav.0);
        super::super::process::capture(
            command,
            control,
            &settings.data_path().join("logs/quality-ffmpeg.log"),
            Duration::from_secs(120),
        )?;
        ensure!(
            media::duration_ms(&settings, &wav.0, control)?.abs_diff(end_ms - start_ms) < 100,
            "局部音频转换不完整"
        );
        let request = transcribe_request(
            &settings,
            &id,
            &wav.0,
            &settings.model,
            device,
            &settings.cache_path("checkpoints").join("rechecks"),
        );
        let result = WhisperWorker {
            path: self.worker_path.clone(),
        }
        .execute(&settings, request, control, &mut |_| Ok(()))?;
        let segments: Vec<Segment> = result["segments"]
            .as_array()
            .context("复核结果缺少文字")?
            .iter()
            .map(|v| -> Result<Segment> {
                let first = v["start_ms"].as_u64().context("无效起点")?;
                let last = v["end_ms"].as_u64().context("无效终点")?;
                ensure!(
                    last > first && last <= end_ms - start_ms,
                    "复核结果超出区间"
                );
                Ok(Segment {
                    id: new_id(),
                    start_ms: start_ms + first,
                    end_ms: start_ms + last,
                    text: v["text"].as_str().context("无效文字")?.into(),
                })
            })
            .collect::<Result<_>>()?;
        ensure!(!segments.is_empty(), "此区间未识别到文字，原文已保留");
        let candidate = RecheckCandidate {
            id,
            asset_id: asset_id.into(),
            transcript_id: transcript_id.into(),
            start_ms,
            end_ms,
            model: settings.model.clone(),
            device: device.into(),
            original_text: base
                .segments
                .iter()
                .filter(|s| s.start_ms < end_ms && s.end_ms > start_ms)
                .map(|s| s.text.as_str())
                .collect::<Vec<_>>()
                .join("\n"),
            segments,
            created_at: now(),
            audio_fingerprint: fingerprint.clone(),
        };
        let _gate = self.mutation_gate.lock().unwrap();
        control.check()?;
        ensure!(
            audio_hash(&path, control)? == fingerprint
                && db.get_asset(asset_id)?.audio_path == asset.audio_path,
            "音频已变化，请重新复核"
        );
        ensure!(
            db.get_asset(asset_id)?.active_version_id.as_deref() == Some(transcript_id),
            "文字版本已变化，请重新复核"
        );
        db.save_candidate(&candidate)?;
        Ok(candidate)
    }
    pub fn list_recheck_candidates(
        &self,
        asset_id: &str,
        transcript_id: &str,
    ) -> Result<Vec<RecheckCandidate>> {
        self.db().list_candidates(asset_id, transcript_id)
    }
    pub fn discard_candidate(&self, candidate_id: &str) -> Result<()> {
        let _gate = self.mutation_gate.lock().unwrap();
        self.db().discard_candidate(candidate_id)
    }
    pub fn adopt_candidate(&self, candidate_id: &str) -> Result<Transcript> {
        let _gate = self.mutation_gate.lock().unwrap();
        let db = self.db();
        let c = db.get_candidate(candidate_id)?;
        let asset = db.get_asset(&c.asset_id)?;
        let path = audio_path(&asset)?;
        ensure!(
            audio_hash(&path, &ProcessControl::default())? == c.audio_fingerprint,
            "音频已变化，不能采纳旧候选"
        );
        let report = self.check_integrity(&c.asset_id, &c.transcript_id)?;
        let issues: Vec<String> = report
            .issues
            .iter()
            .filter(|i| {
                [
                    "head",
                    "gap",
                    "tail",
                    "uncoveredSpeech",
                    "recognitionDoubt",
                    "repetition",
                    "overlap",
                ]
                .contains(&i.code.as_str())
                    && i.start_ms >= c.start_ms
                    && i.end_ms <= c.end_ms
            })
            .map(|i| i.id.clone())
            .collect();
        db.adopt_candidate_with_reviews(candidate_id, &report.fingerprint, &issues)
    }
    fn detector_identity(&self) -> Result<String> {
        let control = Arc::new(ProcessControl::default());
        let id = format!("identity-{}", new_id());
        self.controls
            .lock()
            .unwrap()
            .insert(id.clone(), control.clone());
        let _guard = AuxiliaryGuard {
            runtime: self,
            id: id.clone(),
            control: control.clone(),
            exclusive: false,
        };
        let worker = WhisperWorker {
            path: self.worker_path.with_file_name("quality_worker.py"),
        };
        let result = worker.execute(
            &self.settings(),
            json!({"command":"detector_identity","job_id":id}),
            &control,
            &mut |_| Ok(()),
        )?;
        Ok(result["detector_identity"]
            .as_str()
            .context("检测器身份缺失")?
            .into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temp_audio_removes_failed_conversion_partial() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("vad.wav");
        let partial = path.with_extension("partial.wav");
        fs::write(&path, b"wav").unwrap();
        fs::write(&partial, b"partial").unwrap();
        drop(TempAudio(path.clone()));
        assert!(!path.exists());
        assert!(!partial.exists());
    }
}
