use magi::backend::*;

#[test]
fn test_backend_db() {
    let db = Database::new("/tmp/magi", "optimism-goerli");
    let head = HeadInfo::default();
    db.write_head(head.clone()).unwrap();
    let read_head = db.read_head().unwrap();
    assert_eq!(head, read_head);
    db.clear().unwrap();
}
