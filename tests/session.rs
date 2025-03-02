use std::vec;
use tms::session::Session;
#[cfg(test)]
#[test]
fn verify_session_name_deduplication() {
    let mut test_sessions = vec![
        Session::new(
            "test".into(),
            SessionType::Bookmark("/search/path/to/proj1/test".into()),
        ),
        Session::new(
            "test".into(),
            SessionType::Bookmark("/search/path/to/proj2/test".into()),
        ),
        Session::new(
            "test".into(),
            SessionType::Bookmark("/other/path/to/projects/proj2/test".into()),
        ),
    ];

    let deduplicated = deduplicate_sessions(&mut test_sessions);

    assert_eq!(deduplicated[0].name, "projects/proj2/test");
    assert_eq!(deduplicated[1].name, "to/proj2/test");
    assert_eq!(deduplicated[2].name, "to/proj1/test");
}

#[test]
fn test_merge_session_maps_non_overlapping() {
    let mut map1 = HashMap::new();
    let mut map2 = HashMap::new();

    let session_a = Session::new(
        "Session A".to_string(),
        SessionType::Standard(PathBuf::from("/path/to/a")),
    );
    let session_b = Session::new(
        "Session B".to_string(),
        SessionType::Standard(PathBuf::from("/path/to/b")),
    );

    map1.insert("key1".to_string(), vec![session_a]);
    map2.insert("key2".to_string(), vec![session_b]);

    let merged = merge_session_maps(map1, map2);

    // Expect both keys to exist independently.
    assert_eq!(merged.len(), 2);
    assert!(merged.contains_key("key1"));
    assert!(merged.contains_key("key2"));
    assert_eq!(merged["key1"].len(), 1);
    assert_eq!(merged["key2"].len(), 1);
}

#[test]
fn test_merge_session_maps_overlapping() {
    let mut map1 = HashMap::new();
    let mut map2 = HashMap::new();

    let session_a = Session::new(
        "Session A".to_string(),
        SessionType::Standard(PathBuf::from("/path/to/a")),
    );
    let session_b = Session::new(
        "Session B".to_string(),
        SessionType::Standard(PathBuf::from("/path/to/b")),
    );

    // Both maps have the same key "shared_key"
    map1.insert("shared_key".to_string(), vec![session_a]);
    map2.insert("shared_key".to_string(), vec![session_b]);

    let merged = merge_session_maps(map1, map2);

    // Expect one key "shared_key" with both sessions, map1's session first.
    assert_eq!(merged.len(), 1);
    let sessions = merged.get("shared_key").unwrap();
    assert_eq!(sessions.len(), 2);
    assert_eq!(sessions[0].name, "Session A");
    assert_eq!(sessions[1].name, "Session B");
}

#[test]
fn test_merge_session_maps_empty() {
    let map1: HashMap<String, Vec<Session>> = HashMap::new();
    let map2: HashMap<String, Vec<Session>> = HashMap::new();

    let merged = merge_session_maps(map1, map2);

    // Expect an empty map.
    assert!(merged.is_empty());
}
