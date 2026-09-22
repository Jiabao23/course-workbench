//! Persisted quality observations and explicitly adopted local rechecks.
use crate::{
    db::{index_text, insert_transcript, load_transcript, now},
    subtitles::validate_segments,
    Db, Segment, Transcript,
};
use anyhow::{ensure, Context, Result};
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashSet;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct IssueReview {
    pub issue_id: String,
    pub status: String,
    pub note: String,
    pub reviewed_at: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RecheckCandidate {
    pub id: String,
    pub asset_id: String,
    pub transcript_id: String,
    pub start_ms: u64,
    pub end_ms: u64,
    pub model: String,
    pub device: String,
    pub original_text: String,
    pub segments: Vec<Segment>,
    pub created_at: String,
    pub audio_fingerprint: String,
}

/// A current report issue supplied by the service; core independently restricts
/// automatic manual-edit resolution to text issues covered by changed segments.
#[derive(Clone, Debug)]
pub struct EditIssue {
    pub id: String,
    pub code: String,
    pub start_ms: u64,
    pub end_ms: u64,
}

fn key(value: &str) -> Result<()> {
    ensure!(
        !value.trim().is_empty() && value.len() <= 512,
        "标识不能为空或超过 512 字节"
    );
    Ok(())
}
fn kind_valid(kind: &str) -> Result<()> {
    ensure!(
        ["diagnostics", "speech", "provenance"].contains(&kind),
        "未知质量依据类型"
    );
    Ok(())
}
fn put_evidence(conn: &Connection, id: &str, kind: &str, value: &Value) -> Result<()> {
    kind_valid(kind)?;
    let payload = serde_json::to_string(value)?;
    if kind == "speech" {
        // Archive before replacing under the caller's transaction. Exact retries
        // do not duplicate history or change the original archive timestamp.
        conn.execute("INSERT INTO quality_evidence_history(transcript_id,kind,payload,archived_at) SELECT transcript_id,kind,payload,?4 FROM quality_evidence WHERE transcript_id=?1 AND kind=?2 AND payload<>?3 ON CONFLICT(transcript_id,kind,payload) DO NOTHING",params![id,kind,payload,now()])?;
        conn.execute("INSERT INTO quality_evidence(transcript_id,kind,payload) VALUES(?1,?2,?3) ON CONFLICT(transcript_id,kind) DO UPDATE SET payload=excluded.payload",params![id,kind,payload])?;
    } else {
        conn.execute("INSERT INTO quality_evidence(transcript_id,kind,payload) VALUES(?1,?2,?3) ON CONFLICT(transcript_id,kind) DO NOTHING",params![id,kind,payload])?;
        let stored: String = conn.query_row(
            "SELECT payload FROM quality_evidence WHERE transcript_id=?1 AND kind=?2",
            params![id, kind],
            |r| r.get(0),
        )?;
        ensure!(
            serde_json::from_str::<Value>(&stored)? == *value,
            "该版本的质量依据已保存，不能覆盖"
        );
    }
    Ok(())
}

impl Db {
    pub fn save_issue_review(
        &self,
        transcript_id: &str,
        fingerprint: &str,
        issue_id: &str,
        status: &str,
        note: &str,
    ) -> Result<()> {
        key(fingerprint)?;
        key(issue_id)?;
        ensure!(
            ["pending", "confirmed", "revised"].contains(&status),
            "无效的核对状态"
        );
        ensure!(
            note.chars().count() <= 4000 && (status == "pending" || !note.trim().is_empty()),
            "确认问题需要填写 1–4000 字的核对结论"
        );
        self.connection()?.execute("INSERT INTO issue_reviews(transcript_id,fingerprint,issue_id,status,note,reviewed_at) VALUES(?1,?2,?3,?4,?5,?6) ON CONFLICT(transcript_id,fingerprint,issue_id) DO UPDATE SET status=excluded.status,note=excluded.note,reviewed_at=excluded.reviewed_at",params![transcript_id,fingerprint,issue_id,status,note.trim(),now()])?;
        Ok(())
    }
    pub fn issue_reviews(
        &self,
        transcript_id: &str,
        fingerprint: &str,
    ) -> Result<Vec<IssueReview>> {
        key(fingerprint)?;
        let conn = self.connection()?;
        let mut stmt=conn.prepare("SELECT issue_id,status,note,reviewed_at FROM issue_reviews WHERE transcript_id=?1 AND fingerprint=?2 ORDER BY issue_id")?;
        let rows = stmt
            .query_map(params![transcript_id, fingerprint], |r| {
                Ok(IssueReview {
                    issue_id: r.get(0)?,
                    status: r.get(1)?,
                    note: r.get(2)?,
                    reviewed_at: r.get(3)?,
                })
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }
    pub fn save_quality_evidence(
        &self,
        transcript_id: &str,
        kind: &str,
        value: &Value,
    ) -> Result<()> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        put_evidence(&tx, transcript_id, kind, value)?;
        tx.commit()?;
        Ok(())
    }
    pub fn quality_evidence(&self, transcript_id: &str, kind: &str) -> Result<Option<Value>> {
        kind_valid(kind)?;
        let payload: Option<String> = self
            .connection()?
            .query_row(
                "SELECT payload FROM quality_evidence WHERE transcript_id=?1 AND kind=?2",
                params![transcript_id, kind],
                |r| r.get(0),
            )
            .optional()?;
        payload.map(|v| Ok(serde_json::from_str(&v)?)).transpose()
    }
    pub fn quality_evidence_history(&self, transcript_id: &str, kind: &str) -> Result<Vec<Value>> {
        kind_valid(kind)?;
        let conn = self.connection()?;
        let mut stmt=conn.prepare("SELECT payload FROM quality_evidence_history WHERE transcript_id=?1 AND kind=?2 ORDER BY archived_at,rowid")?;
        let rows = stmt
            .query_map(params![transcript_id, kind], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|s| Ok(serde_json::from_str(&s)?))
            .collect()
    }
    pub fn save_candidate(&self, candidate: &RecheckCandidate) -> Result<()> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let base = load_transcript(&tx, &candidate.transcript_id)?;
        candidate_segments(candidate, &base)?;
        ensure!(base.is_active, "原转写版本已改变，请重新复核");
        let duration: u64 = tx.query_row(
            "SELECT duration_ms FROM assets WHERE id=?1",
            [&candidate.asset_id],
            |r| r.get(0),
        )?;
        ensure!(
            duration == 0 || candidate.end_ms <= duration,
            "复核区间超出课程时长"
        );
        let payload = serde_json::to_string(candidate)?;
        tx.execute("INSERT INTO recheck_candidates(id,asset_id,transcript_id,payload) VALUES(?1,?2,?3,?4) ON CONFLICT(id) DO NOTHING",params![candidate.id,candidate.asset_id,candidate.transcript_id,payload])?;
        ensure!(
            load_candidate(&tx, &candidate.id)? == *candidate,
            "候选标识已用于其他内容"
        );
        tx.commit()?;
        Ok(())
    }
    pub fn get_candidate(&self, id: &str) -> Result<RecheckCandidate> {
        load_candidate(&self.connection()?, id)
    }
    pub fn list_candidates(
        &self,
        asset_id: &str,
        transcript_id: &str,
    ) -> Result<Vec<RecheckCandidate>> {
        let conn = self.connection()?;
        let mut stmt=conn.prepare("SELECT payload FROM recheck_candidates WHERE asset_id=?1 AND transcript_id=?2 AND adopted_transcript_id IS NULL ORDER BY rowid")?;
        let rows = stmt
            .query_map(params![asset_id, transcript_id], |r| r.get::<_, String>(0))?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        rows.into_iter()
            .map(|p| Ok(serde_json::from_str(&p)?))
            .collect()
    }
    pub fn discard_candidate(&self, id: &str) -> Result<()> {
        let conn = self.connection()?;
        let changed = conn.execute(
            "DELETE FROM recheck_candidates WHERE id=?1 AND adopted_transcript_id IS NULL",
            [id],
        )?;
        ensure!(changed == 1, "候选不存在或已经采用");
        Ok(())
    }
    pub fn save_edit_with_reviews(
        &self,
        asset_id: &str,
        base_id: &str,
        segments: &[Segment],
        fingerprint: &str,
        issues: &[EditIssue],
    ) -> Result<Transcript> {
        key(fingerprint)?;
        validate_segments(segments)?;
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let base = load_transcript(&tx, base_id)?;
        ensure!(
            base.asset_id == asset_id && base.is_active,
            "当前文字版本已改变，请重新加载后再保存"
        );
        ensure!(
            base.segments.len() == segments.len(),
            "文字校对必须保留原始片段"
        );
        let mut changed = Vec::new();
        for (old, new) in base.segments.iter().zip(segments) {
            ensure!(
                old.id == new.id && old.start_ms == new.start_ms && old.end_ms == new.end_ms,
                "文字校对必须保留原始片段 ID、顺序和时间轴"
            );
            let meaningful = |text: &str| {
                text.chars()
                    .filter(|c| !c.is_whitespace())
                    .collect::<String>()
            };
            if meaningful(&old.text) != meaningful(&new.text) {
                changed.push(old);
            }
        }
        let addressed: Vec<_> = issues
            .iter()
            .filter(|issue| {
                if !["recognitionDoubt", "repetition"].contains(&issue.code.as_str())
                    || issue.end_ms <= issue.start_ms
                {
                    return false;
                }
                let mut covered = issue.start_ms;
                for segment in &changed {
                    if segment.end_ms <= covered {
                        continue;
                    }
                    if segment.start_ms > covered {
                        break;
                    }
                    covered = covered.max(segment.end_ms);
                    if covered >= issue.end_ms {
                        return true;
                    }
                }
                false
            })
            .collect();
        let transcript = insert_transcript(
            &tx,
            asset_id,
            "edited",
            base.model.as_deref(),
            &base.language,
            segments,
            &index_text(segments),
        )?;
        let provenance = serde_json::json!({"kind":"manualEdit","baseTranscriptId":base.id,"changedSegmentIds":changed.iter().map(|s|&s.id).collect::<Vec<_>>(),"reportFingerprint":fingerprint,"revisedIssueIds":addressed.iter().map(|i|&i.id).collect::<Vec<_>>()});
        put_evidence(&tx, &transcript.id, "provenance", &provenance)?;
        let note = format!("已手动校对相关文字，生成转写版本 {}", transcript.id);
        for issue in addressed {
            key(&issue.id)?;
            tx.execute("INSERT INTO issue_reviews(transcript_id,fingerprint,issue_id,status,note,reviewed_at) VALUES(?1,?2,?3,'revised',?4,?5) ON CONFLICT(transcript_id,fingerprint,issue_id) DO UPDATE SET status=excluded.status,note=excluded.note,reviewed_at=excluded.reviewed_at",params![base.id,fingerprint,issue.id,note,transcript.created_at])?;
        }
        tx.commit()?;
        Ok(transcript)
    }
    pub fn adopt_candidate(&self, id: &str) -> Result<Transcript> {
        self.adopt_candidate_with_reviews(id, "", &[])
    }
    /// The caller supplies only issues fully addressed by this candidate.
    /// Reviews are attached to the base report, never the newly created report.
    pub fn adopt_candidate_with_reviews(
        &self,
        id: &str,
        fingerprint: &str,
        issue_ids: &[String],
    ) -> Result<Transcript> {
        if !issue_ids.is_empty() {
            key(fingerprint)?;
            for issue_id in issue_ids {
                key(issue_id)?;
            }
        }
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let candidate = load_candidate(&tx, id)?;
        let adopted: Option<String> = tx.query_row(
            "SELECT adopted_transcript_id FROM recheck_candidates WHERE id=?1",
            [id],
            |r| r.get(0),
        )?;
        if let Some(adopted) = adopted {
            return load_transcript(&tx, &adopted);
        }
        let base = load_transcript(&tx, &candidate.transcript_id)?;
        ensure!(base.is_active, "原转写版本已改变，请重新复核");
        let segments = candidate_segments(&candidate, &base)?;
        let transcript = insert_transcript(
            &tx,
            &base.asset_id,
            "edited",
            None,
            &base.language,
            &segments,
            &index_text(&segments),
        )?;
        let provenance = serde_json::json!({"baseTranscriptId":base.id,"candidateId":candidate.id,"startMs":candidate.start_ms,"endMs":candidate.end_ms,"model":candidate.model,"device":candidate.device,"audioFingerprint":candidate.audio_fingerprint,"replacedSegmentIds":base.segments.iter().filter(|s|s.start_ms < candidate.end_ms && s.end_ms > candidate.start_ms).map(|s|&s.id).collect::<Vec<_>>()});
        put_evidence(&tx, &transcript.id, "provenance", &provenance)?;
        let note = format!("已采用局部复核结果，生成转写版本 {}", transcript.id);
        for issue_id in issue_ids {
            tx.execute("INSERT INTO issue_reviews(transcript_id,fingerprint,issue_id,status,note,reviewed_at) VALUES(?1,?2,?3,'revised',?4,?5) ON CONFLICT(transcript_id,fingerprint,issue_id) DO UPDATE SET status=excluded.status,note=excluded.note,reviewed_at=excluded.reviewed_at",params![base.id,fingerprint,issue_id,note,transcript.created_at])?;
        }
        tx.execute(
            "UPDATE recheck_candidates SET adopted_transcript_id=?1 WHERE id=?2",
            params![transcript.id, id],
        )?;
        tx.commit()?;
        Ok(transcript)
    }
}

fn load_candidate(conn: &Connection, id: &str) -> Result<RecheckCandidate> {
    let payload: String = conn
        .query_row(
            "SELECT payload FROM recheck_candidates WHERE id=?1",
            [id],
            |r| r.get(0),
        )
        .optional()?
        .context("复核候选不存在")?;
    Ok(serde_json::from_str(&payload)?)
}
fn candidate_segments(c: &RecheckCandidate, base: &Transcript) -> Result<Vec<Segment>> {
    key(&c.id)?;
    key(&c.model)?;
    key(&c.device)?;
    key(&c.audio_fingerprint)?;
    ensure!(
        c.asset_id == base.asset_id && c.transcript_id == base.id,
        "复核候选不属于该课程版本"
    );
    ensure!(
        c.end_ms > c.start_ms && c.end_ms - c.start_ms <= 120_000 && c.end_ms <= i64::MAX as u64,
        "复核区间应在 120 秒以内"
    );
    validate_segments(&c.segments)?;
    let old_ids: HashSet<_> = base.segments.iter().map(|s| s.id.as_str()).collect();
    for s in &c.segments {
        ensure!(
            s.start_ms >= c.start_ms && s.end_ms <= c.end_ms,
            "候选字幕超出复核区间"
        );
        ensure!(
            !old_ids.contains(s.id.as_str()),
            "候选字幕必须使用新的片段标识"
        );
    }
    let mut segments = Vec::new();
    for s in &base.segments {
        if s.start_ms < c.end_ms && s.end_ms > c.start_ms {
            ensure!(
                s.start_ms >= c.start_ms && s.end_ms <= c.end_ms,
                "复核区间不能切断原字幕片段"
            );
        } else {
            segments.push(s.clone());
        }
    }
    segments.extend(c.segments.iter().cloned());
    segments.sort_by_key(|s| s.start_ms);
    validate_segments(&segments)?;
    Ok(segments)
}
