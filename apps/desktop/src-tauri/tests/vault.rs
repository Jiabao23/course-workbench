use course_core::{Asset, Citation, Note, Segment, Transcript};
use course_workbench_lib::{integrity::check, vault};
use std::fs;
use tempfile::TempDir;

fn sample() -> (Asset, Transcript, Note) {
    let a:Asset=serde_json::from_value(serde_json::json!({"id":"asset-1","title":"中文 / 同名 # 课程","sourceKind":"webMedia","source":"https://example.org/video","bvid":null,"page":1,"durationMs":2000,"audioPath":null,"activeVersionId":"version-1","createdAt":"now","updatedAt":"now"})).unwrap();
    let t = Transcript {
        id: "version-1".into(),
        asset_id: a.id.clone(),
        version: 1,
        source_kind: "webSubtitle".into(),
        model: None,
        language: "zh".into(),
        segments: vec![Segment {
            id: "segment-1".into(),
            start_ms: 0,
            end_ms: 2000,
            text: "专业术语与引用".into(),
        }],
        created_at: "now".into(),
        is_active: true,
    };
    let n = Note {
        id: "note-1".into(),
        asset_id: a.id.clone(),
        transcript_id: t.id.clone(),
        kind: "manual".into(),
        title: "理解".into(),
        content: "[引用:segment-1] 我的理解".into(),
        citations: vec![Citation {
            segment_id: "segment-1".into(),
            start_ms: 0,
            end_ms: 2000,
            text: "专业术语与引用".into(),
        }],
        question: None,
        created_at: "now".into(),
        stale: false,
    };
    (a, t, n)
}
#[test]
fn local_vault_roundtrip_is_idempotent_and_keeps_personal_edits() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("个人 知识库");
    vault::initialize(&root).unwrap();
    let (a, t, n) = sample();
    let r = check(&a, &t, None, None);
    let first = vault::sync(&root, &a, &t, std::slice::from_ref(&n), &r).unwrap();
    fs::write(&first.personal_path, "用户自己的补充\n").unwrap();
    let second = vault::sync(&root, &a, &t, std::slice::from_ref(&n), &r).unwrap();
    assert_eq!(first.snapshot_path, second.snapshot_path);
    assert!(!second.created);
    assert_eq!(
        fs::read_to_string(&first.personal_path).unwrap(),
        "用户自己的补充\n"
    );
    let text = fs::read_to_string(&first.snapshot_path).unwrap();
    let block = vault::block_id("segment-1");
    assert!(text.contains(&format!("^{block}")));
    assert!(text.contains(&format!("#^{block}")));
    let mut updated = n;
    updated.content.push_str(" 新笔记");
    let third = vault::sync(&root, &a, &t, &[updated], &r).unwrap();
    assert_ne!(first.snapshot_path, third.snapshot_path);
    assert!(fs::metadata(&first.snapshot_path).is_ok());
    let index = fs::read_to_string(&first.index_path).unwrap();
    assert_eq!(index.matches(&first.snapshot_link).count(), 1);
    assert!(index.contains(&third.snapshot_link));
    let url = url::Url::parse(&first.open_uri).unwrap();
    assert_eq!(url.scheme(), "obsidian");
    assert_eq!(url.host_str(), Some("open"));
    assert_eq!(
        url.query_pairs().find(|(k, _)| k == "path").unwrap().1,
        first.index_path
    );
}
#[test]
fn user_modified_snapshot_is_never_overwritten() {
    let temp = TempDir::new().unwrap();
    vault::initialize(temp.path()).unwrap();
    let (a, t, n) = sample();
    let r = check(&a, &t, None, None);
    let first = vault::sync(temp.path(), &a, &t, std::slice::from_ref(&n), &r).unwrap();
    fs::write(&first.snapshot_path, "我修改了快照").unwrap();
    assert!(vault::sync(temp.path(), &a, &t, &[n], &r)
        .unwrap_err()
        .to_string()
        .contains("冲突"));
    assert_eq!(
        fs::read_to_string(first.snapshot_path).unwrap(),
        "我修改了快照"
    );
}
#[test]
fn missing_vault_invalid_ids_and_foreign_notes_are_rejected() {
    let temp = TempDir::new().unwrap();
    let (mut a, t, mut n) = sample();
    let r = check(&a, &t, None, None);
    assert!(vault::sync(temp.path(), &a, &t, std::slice::from_ref(&n), &r).is_err());
    vault::initialize(temp.path()).unwrap();
    a.id = "../escape".into();
    assert!(vault::sync(temp.path(), &a, &t, &[], &r).is_err());
    let (a, _, _) = sample();
    n.transcript_id = "another-version".into();
    assert!(vault::sync(temp.path(), &a, &t, &[n], &r).is_err());
}
#[cfg(windows)]
#[test]
fn junction_cannot_redirect_exports_outside_vault() {
    let temp = TempDir::new().unwrap();
    let root = temp.path().join("vault");
    let outside = temp.path().join("outside");
    fs::create_dir_all(root.join(".obsidian")).unwrap();
    fs::create_dir(&outside).unwrap();
    let status = std::process::Command::new("cmd")
        .args(["/C", "mklink", "/J"])
        .arg(root.join("CourseWorkbench"))
        .arg(&outside)
        .output()
        .unwrap();
    assert!(status.status.success());
    let (a, t, n) = sample();
    let r = check(&a, &t, None, None);
    assert!(vault::sync(&root, &a, &t, &[n], &r).is_err());
    assert_eq!(fs::read_dir(outside).unwrap().count(), 0);
}
