use serde_json::{Value, json};
use snowify::local_storage::{self, Storage};
mod common;
fn pairs(map: &Storage) -> Value {
    json!(map.iter().map(|(k, v)| (k, v)).collect::<Vec<_>>())
}
fn fixture() -> Storage {
    let path = common::root().join("parity/corpus/local_storage");
    let expected: Vec<(Vec<u16>,Vec<u16>)>=serde_json::from_reader(flate2::read::GzDecoder::new(std::fs::File::open(path.join("expected.json.gz")).expect("generate fixture: node_modules/.bin/electron src-tauri/parity/node/local_storage_fixture.js"))).unwrap();
    expected.into_iter().collect()
}
#[test]
fn local_storage_matches_electron() {
    let expected = fixture();
    let path = common::root().join("parity/corpus/local_storage/leveldb");
    let before = tree(&path);
    let actual = local_storage::read_leveldb(&path).unwrap();
    assert_eq!(before, tree(&path), "reader changed source DB");
    assert_eq!(pairs(&expected), pairs(&actual));
    println!(
        "local_storage parity: compared {}, differences 0",
        expected.len()
    );
}
fn tree(path: &std::path::Path) -> std::collections::BTreeMap<String, Vec<u8>> {
    std::fs::read_dir(path)
        .unwrap()
        .map(|e| {
            let e = e.unwrap();
            (
                e.file_name().to_string_lossy().into_owned(),
                std::fs::read(e.path()).unwrap(),
            )
        })
        .collect()
}
fn check_injection(map: Storage) {
    let result = common::run_source(
        "local_storage.js",
        &json!({"script":local_storage::injection_script(&map),"marker":local_storage::MARKER}),
    );
    let mut restored: Storage =
        serde_json::from_value::<Vec<(Vec<u16>, Vec<u16>)>>(result["empty"]["pairs"].clone())
            .unwrap()
            .into_iter()
            .collect();
    assert_eq!(
        restored.remove(&local_storage::MARKER.encode_utf16().collect::<Vec<_>>()),
        Some(vec![49])
    );
    assert_eq!(map, restored);
    assert_eq!(result["windows"]["pairs"], result["empty"]["pairs"]);
    for guard in result["guards"].as_array().unwrap() {
        assert_eq!(guard["unchanged"], true);
    }
    assert_eq!(
        result["failure"]["unchanged"], true,
        "failed migration must roll back for retry"
    );
}
#[test]
fn injection_fixture() {
    check_injection(fixture());
}
#[test]
fn injection_edge_cases_without_fixture() {
    let mut map = Storage::new();
    map.insert(vec![0, 0xd800], vec![0xdfff, 0, 0x2028, 0x2029]);
    map.insert(
        "ascii".encode_utf16().collect(),
        "</script>\"\\\n".encode_utf16().collect(),
    );
    map.insert(vec![], vec![]);
    map.insert(vec![0xff], vec![120; 5 * 1024 * 1024]);
    check_injection(map);
}
