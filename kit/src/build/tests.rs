use super::BuildIdentity;

#[test]
fn a_label_names_the_commit_and_marks_local_changes() {
    assert_eq!(BuildIdentity::new("1", "abc123", "false").label(), "abc123");
    assert_eq!(
        BuildIdentity::new("1", "abc123", "true").label(),
        "abc123-dirty"
    );
    assert_eq!(
        BuildIdentity::new("1", "abc123", "unknown").label(),
        "abc123"
    );
}
