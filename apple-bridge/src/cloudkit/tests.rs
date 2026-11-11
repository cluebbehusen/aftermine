use super::bridge::answer;
use super::*;

fn refused(reply: &str) -> io::Error {
    answer::<serde_json::Value>(Some(reply.as_bytes().to_vec())).unwrap_err()
}

#[test]
fn a_refusal_carries_its_trouble_and_the_wait_cloudkit_asked_for() {
    let error =
        refused(r#"{"error": "Quota exceeded", "trouble": "storage_full", "retry_after": 2.5}"#);
    assert_eq!(
        Failure::of(&error),
        Some(&Failure {
            trouble: Trouble::StorageFull,
            retry_after: Some(Duration::from_millis(2_500)),
            message: "Quota exceeded".to_owned(),
        })
    );
    assert_eq!(error.to_string(), "Quota exceeded");
    let error = refused(r#"{"error": "Not signed in", "trouble": "no_account"}"#);
    assert_eq!(Failure::of(&error).unwrap().trouble, Trouble::NoAccount);
    assert_eq!(Failure::of(&error).unwrap().retry_after, None);
}

#[test]
fn a_refusal_that_names_no_trouble_is_failing() {
    for error in [
        refused(r#"{"error": "CloudKit did not answer in time"}"#),
        refused(r#"{"error": "busy", "retry_after": 0}"#),
        refused(r#"{"error": "busy", "retry_after": -3}"#),
        answer::<serde_json::Value>(None).unwrap_err(),
        answer::<Changes>(Some(b"{}".to_vec())).unwrap_err(),
    ] {
        let failure = Failure::of(&error).expect("CloudKit's refusal");
        assert_eq!(failure.trouble, Trouble::Failing, "{error}");
        assert_eq!(failure.retry_after, None, "{error}");
    }
}

#[test]
fn a_version_further_along_is_no_trouble() {
    let error = refused(r#"{"error": "CloudKit holds a version further along", "behind": true}"#);
    assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
    assert_eq!(Failure::of(&error), None);
}

#[test]
fn an_answer_reads_as_its_type() {
    let changes: Changes = answer(Some(
        br#"{"changed": [], "token": "t", "more": false, "failed": 0}"#.to_vec(),
    ))
    .unwrap();
    assert_eq!(changes.token.as_deref(), Some("t"));
}
