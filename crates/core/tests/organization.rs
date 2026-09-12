mod common;
use common::{asset, segment};
use course_core::Db;

fn db() -> (tempfile::TempDir, Db) {
    let dir = tempfile::tempdir().unwrap();
    let db = Db::open(&dir.path().join("library.sqlite3")).unwrap();
    for id in ["a", "b"] {
        db.upsert_asset(&asset(id)).unwrap();
    }
    (dir, db)
}
#[test]
fn organize_and_favorite_survive_reopen_without_changing_transcript() {
    let (dir, db) = db();
    let t = db
        .save_transcript(
            "a",
            "subtitle",
            None,
            "zh",
            &[segment("s", 0, 1000, "原文")],
        )
        .unwrap();
    let kb = db.create_collection("  计算机学习  ", None).unwrap();
    let folder = db.create_collection("网络", Some(&kb.id)).unwrap();
    db.move_assets(&["a".into(), "b".into()], Some(&folder.id))
        .unwrap();
    db.set_favorite("a", true).unwrap();
    db.rename_collection(&folder.id, "计算机网络").unwrap();
    let again = Db::open(&dir.path().join("library.sqlite3")).unwrap();
    let state = again.organization().unwrap();
    assert_eq!(
        state
            .collections
            .iter()
            .find(|c| c.id == kb.id)
            .unwrap()
            .name,
        "计算机学习"
    );
    assert_eq!(
        state
            .entries
            .iter()
            .find(|e| e.asset_id == "a")
            .unwrap()
            .collection_id
            .as_deref(),
        Some(folder.id.as_str())
    );
    assert!(
        state
            .entries
            .iter()
            .find(|e| e.asset_id == "a")
            .unwrap()
            .favorite
    );
    assert_eq!(again.get_transcript(&t.id).unwrap(), t);
    again.move_assets(&["a".into()], None).unwrap();
    let entry = again
        .organization()
        .unwrap()
        .entries
        .into_iter()
        .find(|e| e.asset_id == "a")
        .unwrap();
    assert!(entry.favorite);
    assert!(entry.collection_id.is_none());
    again.set_favorite("a", false).unwrap();
    assert!(
        !again
            .organization()
            .unwrap()
            .entries
            .iter()
            .find(|e| e.asset_id == "a")
            .unwrap()
            .favorite
    );
}
#[test]
fn invalid_batch_moves_are_atomic_and_missing_assets_cannot_be_favorited() {
    let (_dir, db) = db();
    let group = db.create_collection("课程", None).unwrap();
    assert!(db
        .move_assets(&["a".into(), "missing".into()], Some(&group.id))
        .is_err());
    assert!(db.organization().unwrap().entries.is_empty());
    assert!(db.move_assets(&["a".into()], Some("missing")).is_err());
    assert!(db.set_favorite("missing", true).is_err());
    assert!(db.move_assets(&[], None).is_err());
}
#[test]
fn classification_names_and_empty_deletion_are_checked() {
    let (_dir, db) = db();
    let root = db.create_collection("Study", None).unwrap();
    assert!(db.create_collection(" study ", None).is_err());
    for name in ["", "  ", "a\nb"] {
        assert!(db.create_collection(name, None).is_err());
    }
    assert!(db.create_collection("child", Some("missing")).is_err());
    let one = db.create_collection("网络", Some(&root.id)).unwrap();
    let two = db.create_collection("协议", Some(&root.id)).unwrap();
    assert!(db.rename_collection(&two.id, "网络").is_err());
    assert!(db.delete_collection(&root.id).is_err());
    db.move_assets(&["a".into()], Some(&one.id)).unwrap();
    assert!(db.delete_collection(&one.id).is_err());
    db.move_assets(&["a".into()], None).unwrap();
    db.delete_collection(&one.id).unwrap();
    db.delete_collection(&two.id).unwrap();
    db.delete_collection(&root.id).unwrap();
    assert_eq!(db.list_assets().unwrap().len(), 2);
    assert!(db.delete_collection("missing").is_err());
}
#[test]
fn nesting_is_bounded_and_duplicate_names_in_other_parents_are_allowed() {
    let (_dir, db) = db();
    let root = db.create_collection("学习", None).unwrap();
    let other = db.create_collection("工作", None).unwrap();
    db.create_collection("网络", Some(&root.id)).unwrap();
    db.create_collection("网络", Some(&other.id)).unwrap();
    let mut parent = root;
    for _ in 1..8 {
        parent = db.create_collection("子目录", Some(&parent.id)).unwrap();
    }
    assert!(db.create_collection("过深", Some(&parent.id)).is_err());
}
#[test]
fn schema_three_upgrade_keeps_existing_content_and_starts_unclassified() {
    let (dir, db) = db();
    let c = rusqlite::Connection::open(db.path()).unwrap();
    c.execute_batch(
        "DROP TABLE asset_organization; DROP TABLE collections; PRAGMA user_version=3;",
    )
    .unwrap();
    drop(c);
    let upgraded = Db::open(&dir.path().join("library.sqlite3")).unwrap();
    assert_eq!(upgraded.list_assets().unwrap().len(), 2);
    assert!(upgraded.organization().unwrap().collections.is_empty());
    assert!(upgraded.organization().unwrap().entries.is_empty());
}
