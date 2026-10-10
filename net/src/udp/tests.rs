use super::*;
use crate::discovery::Heard;
use crate::name::PlayerName;
use std::time::Instant;

const COMMIT: &str = "abc";

fn beacon() -> Beacon {
    Beacon::new(
        COMMIT,
        PlayerName::new("Connor").unwrap(),
        4321,
        "Broad Front",
        2,
        4,
    )
}

#[test]
fn an_advertiser_reaches_a_scanner_once_per_interval() {
    let mut scanner = Scanner::bind_to("127.0.0.1:0", COMMIT).unwrap();
    let mut advertiser = Advertiser::with_target(scanner.local_addr().unwrap()).unwrap();
    advertiser.poll(&beacon(), Duration::ZERO);
    assert!(!advertiser.failing());
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    loop {
        let heard = scanner.poll(Duration::ZERO);
        if let Some(sighting) = heard.first() {
            assert_eq!(sighting.heard, Heard::Joinable(beacon()));
            assert_eq!(sighting.address().unwrap().port(), 4321);
            break;
        }
        assert!(Instant::now() < deadline, "the beacon never arrived");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    // Nothing more goes out within the interval, so the lobby expires.
    advertiser.poll(&beacon(), BEACON_INTERVAL / 2);
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(scanner.poll(crate::discovery::BEACON_EXPIRY).is_empty());
    advertiser.poll(&beacon(), crate::discovery::BEACON_EXPIRY);
    let deadline = Instant::now() + std::time::Duration::from_secs(10);
    while scanner.poll(crate::discovery::BEACON_EXPIRY).is_empty() {
        assert!(Instant::now() < deadline, "the next beacon never arrived");
        std::thread::sleep(std::time::Duration::from_millis(1));
    }
}

#[test]
fn a_scanner_ignores_datagrams_that_are_not_beacons() {
    let mut scanner = Scanner::bind_to("127.0.0.1:0", COMMIT).unwrap();
    let sender = UdpSocket::bind("127.0.0.1:0").unwrap();
    sender
        .send_to(b"hello", scanner.local_addr().unwrap())
        .unwrap();
    std::thread::sleep(std::time::Duration::from_millis(50));
    assert!(scanner.poll(Duration::ZERO).is_empty());
}

#[test]
fn a_second_scanner_on_a_taken_port_reports_the_error() {
    let first = Scanner::bind_to("127.0.0.1:0", COMMIT).unwrap();
    assert!(Scanner::bind_to(first.local_addr().unwrap(), COMMIT).is_err());
}
