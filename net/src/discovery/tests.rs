use super::*;
use std::net::Ipv4Addr;

const COMMIT: &str = "abc";

fn beacon(host: &str, port: u16) -> Beacon {
    Beacon::new(
        COMMIT,
        PlayerName::new(host).unwrap(),
        port,
        "Broad Front",
        2,
        4,
    )
}

fn ip(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(192, 168, 1, last))
}

fn secs(seconds: u64) -> Duration {
    Duration::from_secs(seconds)
}

#[test]
fn a_beacon_of_this_build_is_joinable_and_fits_a_datagram() {
    let sent = beacon("Connor", 4200);
    let datagram = sent.encode();
    assert!(datagram.len() <= MAX_BEACON_BYTES);
    assert_eq!(hear(&datagram, COMMIT), Some(Heard::Joinable(sent)));

    let long = Beacon::new(
        COMMIT,
        PlayerName::new("x".repeat(PlayerName::MAX_CHARS).as_str()).unwrap(),
        4200,
        &"m".repeat(400),
        16,
        300,
    );
    assert_eq!(long.map.chars().count(), 48, "a long map name is cut");
    assert_eq!(long.seats, u8::MAX);
    assert!(long.encode().len() <= MAX_BEACON_BYTES);
}

#[test]
fn another_builds_beacon_lists_by_its_header_and_noise_is_ignored() {
    let theirs = String::from_utf8(beacon("Sam", 4200).encode())
        .unwrap()
        .replace("\"commit\":\"abc\"", "\"commit\":\"def\"")
        .replace("\"map\":", "\"extra\":1,\"map\":");
    assert_eq!(
        hear(theirs.as_bytes(), COMMIT),
        Some(Heard::OtherBuild {
            host: "Sam".to_owned(),
            commit: "def".to_owned()
        }),
        "a later build may add fields; its header still reads"
    );
    let not_ours = String::from_utf8(beacon("Sam", 4200).encode())
        .unwrap()
        .replace("aftermine-lan", "other-game");
    for datagram in [
        not_ours.as_bytes(),
        b"not json".as_slice(),
        b"{}".as_slice(),
        &[b' '; MAX_BEACON_BYTES + 1],
    ] {
        assert_eq!(hear(datagram, COMMIT), None);
    }
    let bad_name = String::from_utf8(beacon("Sam", 4200).encode())
        .unwrap()
        .replace("\"Sam\"", "\"\"");
    assert_eq!(
        hear(bad_name.as_bytes(), COMMIT),
        None,
        "a bad name in our own build"
    );
}

#[test]
fn sightings_update_in_place_sort_by_host_and_expire() {
    let mut list = Sightings::default();
    list.hear(ip(2), Heard::Joinable(beacon("Sam", 4200)), secs(0));
    list.hear(ip(3), Heard::Joinable(beacon("Alex", 4200)), secs(1));
    list.hear(ip(2), Heard::Joinable(beacon("Sam", 4200)), secs(2));
    let hosts: Vec<&str> = list.list().iter().map(Sighting::host).collect();
    assert_eq!(hosts, ["Alex", "Sam"], "one entry per lobby, sorted");
    assert_eq!(list.list()[1].address(), Some(SocketAddr::new(ip(2), 4200)));
    list.hear(ip(2), Heard::Joinable(beacon("Sam", 4300)), secs(2));
    assert_eq!(list.list().len(), 3, "another port is another lobby");

    list.expire(secs(4));
    let hosts: Vec<&str> = list.list().iter().map(Sighting::host).collect();
    assert_eq!(hosts, ["Sam", "Sam"], "Alex fell silent");
    list.expire(secs(5));
    assert!(list.list().is_empty());

    let other = Heard::OtherBuild {
        host: "Riley".to_owned(),
        commit: "def".to_owned(),
    };
    list.hear(ip(4), other, secs(5));
    assert_eq!(
        list.list()[0].address(),
        None,
        "another build cannot be joined"
    );
}

#[test]
fn the_list_is_capped() {
    let mut list = Sightings::default();
    for port in 0..u16::try_from(MAX_SIGHTINGS + 5).unwrap() {
        list.hear(ip(2), Heard::Joinable(beacon("Sam", port + 1)), secs(0));
    }
    assert_eq!(list.list().len(), MAX_SIGHTINGS);
}
