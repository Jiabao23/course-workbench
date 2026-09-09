mod common;

use common::{asset, job, segment, transcript};

#[test]
fn public_records_follow_camel_case_contract_and_keep_null_options() {
    let asset = serde_json::to_value(asset("a")).unwrap();
    assert_eq!(asset["sourceKind"], "local");
    assert!(asset.get("activeVersionId").unwrap().is_null());
    assert!(asset.get("audioPath").unwrap().is_null());
    assert!(asset.get("source_kind").is_none());
    let job = serde_json::to_value(job("j", "a", "running")).unwrap();
    assert_eq!(job["chunkDone"], 2);
    assert_eq!(job["chunkTotal"], 5);
    let transcript =
        serde_json::to_value(transcript(vec![segment("s", 1234, 5678, "原文")])).unwrap();
    assert_eq!(transcript["isActive"], true);
    assert!(transcript["model"].is_null());
    assert_eq!(transcript["segments"][0]["startMs"], 1234);
    assert_eq!(transcript["segments"][0]["endMs"], 5678);
}
