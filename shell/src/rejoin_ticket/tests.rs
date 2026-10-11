use super::*;

pub(crate) fn ticket(address: &str, token: Token) -> Ticket {
    Ticket {
        address: address.to_owned(),
        host: "Connor".to_owned(),
        map: "Broad Front".to_owned(),
        seat: 1,
        token,
        build: crate::build_identity().revision,
        contact: now(),
    }
}

fn scratch(name: &str) -> PathBuf {
    let path = std::env::temp_dir().join(format!(
        "oxide-ticket-{name}-{}-{}",
        std::process::id(),
        crate::entropy::bits()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

fn eventually(done: impl Fn() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(10);
    while !done() {
        assert!(Instant::now() < deadline, "timed out");
        thread::sleep(Duration::from_millis(2));
    }
}

#[test]
fn a_ticket_names_its_match_and_expires_after_an_hour_unheard() {
    let ticket = ticket("192.168.1.20:4200", Token::new(9));
    assert_eq!(ticket.label(), "Rejoin Connor's match on Broad Front");
    assert!(!ticket.expired(ticket.contact + EXPIRY.as_secs() - 1));
    assert!(ticket.expired(ticket.contact + EXPIRY.as_secs()));
    let json = serde_json::to_string(&ticket).unwrap();
    assert_eq!(serde_json::from_str::<Ticket>(&json).unwrap(), ticket);
}

#[test]
fn the_keeper_writes_refreshes_and_clears_the_ticket_off_the_frame_loop() {
    let path = scratch("keeper");
    let mut keeper = Keeper::new(Some(path.clone()));
    let mut kept = ticket("127.0.0.1:4200", Token::new(1));
    kept.contact = 0;
    keeper.keep(kept.clone(), Duration::ZERO);
    eventually(|| read(&path).is_some());
    let written = read(&path).unwrap();
    assert_eq!(written.token, Token::new(1));
    assert!(written.contact > 0, "the keeper stamps the contact");
    keeper.clear();
    eventually(|| !path.exists());
    keeper.clear();
    let mut nothing = Keeper::new(None);
    nothing.keep(kept, Duration::ZERO);
    nothing.clear();
}

#[test]
fn a_probe_without_a_live_ticket_offers_nothing() {
    let missing = scratch("missing");
    assert_eq!(
        probe(missing, crate::build_identity().revision)
            .join()
            .unwrap(),
        Probed::Missing
    );

    let stale = scratch("stale");
    let mut old = ticket("127.0.0.1:9", Token::new(1));
    old.contact = 0;
    std::fs::write(&stale, serde_json::to_vec(&old).unwrap()).unwrap();
    assert_eq!(
        probe(stale.clone(), crate::build_identity().revision)
            .join()
            .unwrap(),
        Probed::Gone
    );
    assert!(!stale.exists(), "an expired ticket is deleted");

    let elsewhere = scratch("elsewhere");
    let mut other = ticket("127.0.0.1:9", Token::new(1));
    other.build = "another build".to_owned();
    std::fs::write(&elsewhere, serde_json::to_vec(&other).unwrap()).unwrap();
    assert_eq!(
        probe(elsewhere.clone(), crate::build_identity().revision)
            .join()
            .unwrap(),
        Probed::Gone
    );
    assert!(!elsewhere.exists());
}

#[test]
fn a_probe_forgets_only_the_ticket_it_asked_after() {
    let path = scratch("replaced");
    let old = ticket("127.0.0.1:9", Token::new(1));
    let new = ticket("127.0.0.1:9", Token::new(2));
    std::fs::write(&path, serde_json::to_vec(&new).unwrap()).unwrap();
    forget(&path, &old);
    assert_eq!(
        read(&path),
        Some(new.clone()),
        "a newer match's ticket stays"
    );
    forget(&path, &new);
    assert!(!path.exists());
}
