use super::*;
use macroquad::prelude::vec2;
use oxide_sim::scenario::{BotConfig, BotDifficulty, BotStance};
use oxide_sim::{BuildingId, Command, PlayerCommand, UnitKind};
use std::time::Instant;

const COMMIT: &str = "lan-test";
const STEP: Duration = Duration::from_millis(50);
const HOST: PlayerId = PlayerId(0);
const CLIENT: PlayerId = PlayerId(1);

fn viewport() -> Vec2 {
    vec2(1280.0, 800.0)
}

/// Twin Forges with seats 0 and 1 human and seats 2 and 3 host bots.
fn with_bots() -> Scenario {
    let mut scenario = Scenario::load(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../scenarios/twin-forges.json"
    ))
    .unwrap();
    for (seat, player) in scenario.players.iter_mut().enumerate() {
        player.bot = seat >= 2;
        player.bot_config = player
            .bot
            .then(|| BotConfig::new(BotDifficulty::Standard, BotStance::Balanced, seat as u64));
    }
    scenario
}

/// Skirmish with both seats human.
fn duel() -> Scenario {
    let mut scenario = Scenario::skirmish();
    for player in &mut scenario.players {
        player.bot = false;
        player.bot_config = None;
    }
    scenario
}

fn foundry(game: &Game, seat: PlayerId) -> BuildingId {
    game.state
        .buildings()
        .iter()
        .find(|building| building.player == seat)
        .expect("every seat starts with a Foundry")
        .id
}

fn train(game: &Game, seat: PlayerId) -> Command {
    Command::Train {
        building: foundry(game, seat),
        kind: UnitKind::Harvester,
    }
}

fn recorded(game: &Game) -> Vec<(u64, oxide_sim::PlayerCommand)> {
    game.recorder
        .commands
        .iter()
        .map(|timed| (timed.tick, timed.command.clone()))
        .collect()
}

fn name(name: &str) -> PlayerName {
    PlayerName::new(name).unwrap()
}

/// Twin Forges with seats 0 to 2 human and seat 3 a bot, so one joiner
/// leaves a seat open and the lobby keeps waiting.
fn three_humans() -> Scenario {
    let mut scenario = with_bots();
    scenario.players[2].bot = false;
    scenario.players[2].bot_config = None;
    scenario
}

fn host_lobby(scenario: Scenario) -> Lobby {
    Lobby::Host(Box::new(
        ScenarioHost::new("127.0.0.1:0", scenario, HOST, COMMIT, Some(name("Host"))).unwrap(),
    ))
}

fn address(lobby: &Lobby) -> String {
    match lobby {
        Lobby::Host(host) => host.lobby.address().to_owned(),
        Lobby::Client(_) => unreachable!("a host lobby"),
    }
}

/// Polls `poll` until it yields, failing after ten seconds.
fn wait<T>(mut poll: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(value) = poll() {
            return value;
        }
        assert!(Instant::now() < deadline, "timed out");
        thread::sleep(Duration::from_millis(1));
    }
}

/// One host and one client, stepped on a synthetic clock over real
/// localhost sockets.
struct Match {
    now: Duration,
    host: (Game, Link),
    client: (Game, Link),
}

impl Match {
    fn start(scenario: Scenario) -> Self {
        Self::start_as(scenario, HOST)
    }

    fn start_as(scenario: Scenario, seat: PlayerId) -> Self {
        let mut host = Lobby::Host(Box::new(
            ScenarioHost::new("127.0.0.1:0", scenario, seat, COMMIT, Some(name("Host"))).unwrap(),
        ));
        let mut client = Lobby::Client(ClientLobby::new(&address(&host), COMMIT, name("Alex")));
        let (mut hosted, mut joined) = (None, None);
        wait(|| {
            if hosted.is_none() {
                hosted = host.poll(Duration::ZERO, viewport());
            }
            if joined.is_none() {
                joined = client.poll(Duration::ZERO, viewport());
            }
            (hosted.is_some() && joined.is_some()).then_some(())
        });
        let (Some(host), Some(client)) = (hosted, joined) else {
            unreachable!("both lobbies started");
        };
        Self {
            now: Duration::ZERO,
            host,
            client,
        }
    }

    /// Advances both machines by one tick of time.
    fn step(&mut self) -> (Option<End>, Option<End>) {
        self.now += STEP;
        thread::sleep(Duration::from_millis(1));
        let host = self.host.1.pump(&mut self.host.0, self.now);
        let client = self.client.1.pump(&mut self.client.0, self.now);
        (host, client)
    }

    /// Advances the host alone by one tick of time.
    fn step_host(&mut self) {
        self.now += STEP;
        assert_eq!(self.host.1.pump(&mut self.host.0, self.now), None);
    }

    /// Steps both machines until each has ended.
    fn ends(&mut self) -> (End, End) {
        let (mut host_end, mut client_end) = (None, None);
        wait(|| {
            let (host, client) = self.step();
            host_end = host_end.or(host);
            client_end = client_end.or(client);
            host_end.zip(client_end)
        })
    }

    fn run(&mut self, steps: usize) {
        for _ in 0..steps {
            assert_eq!(self.step(), (None, None));
        }
    }

    /// Pauses the host and lets the client execute what is in flight.
    fn drain(&mut self) {
        self.host.0.clock.paused = true;
        wait(|| {
            assert_eq!(self.step(), (None, None));
            (self.client.0.state.current_tick() == self.host.0.state.current_tick()).then_some(())
        });
    }
}

#[test]
fn a_hosted_match_with_bots_stays_in_sync() {
    let mut lan = Match::start(with_bots());
    assert_eq!(lan.client.0.presentation.human, CLIENT);
    lan.run(10);
    let order = train(&lan.client.0, CLIENT);
    lan.client.0.issue(order.clone());
    let host_order = train(&lan.host.0, HOST);
    lan.host.0.issue(host_order);
    lan.run(70);
    lan.drain();
    assert!(lan.host.0.state.current_tick() >= 60);
    assert_eq!(lan.client.0.state.hash(), lan.host.0.state.hash());
    assert_eq!(recorded(&lan.client.0), recorded(&lan.host.0));
    assert_eq!(lan.client.0.demo, lan.host.0.demo);
    assert!(
        recorded(&lan.host.0)
            .iter()
            .any(|(_, command)| command.player == CLIENT && command.command == order),
        "the client's order crossed the wire"
    );
    assert!(
        lan.client.0.pending.is_empty(),
        "the executed order retired"
    );
}

#[test]
fn a_lost_client_makes_the_host_wait_until_it_drops_the_seat() {
    let mut lan = Match::start(with_bots());
    lan.run(3);
    let Match {
        mut now,
        host: (mut host, mut link),
        client,
    } = lan;
    drop(client);
    wait(|| {
        now += STEP;
        assert_eq!(link.pump(&mut host, now), None);
        (!link.status(now).waiting.is_empty()).then_some(())
    });
    assert_eq!(link.status(now).waiting[0].0, CLIENT);
    assert_eq!(
        host.presentation.net_notice.as_deref(),
        Some("Waiting for Alex")
    );
    let held = host.state.current_tick();
    for _ in 0..20 {
        now += STEP;
        assert_eq!(link.pump(&mut host, now), None);
    }
    assert_eq!(host.state.current_tick(), held, "the match waits");
    assert!(host.state.accepts_commands(CLIENT), "nobody surrendered");

    link.drop_seat(CLIENT);
    wait(|| {
        now += STEP;
        assert_eq!(link.pump(&mut host, now), None);
        (!host.state.accepts_commands(CLIENT)).then_some(())
    });
    assert!(link.status(now).waiting.is_empty());
    assert!(host.presentation.net_notice.is_none());
    let resumed = host.state.current_tick();
    for _ in 0..5 {
        now += STEP;
        assert_eq!(link.pump(&mut host, now), None);
    }
    assert!(host.state.current_tick() > resumed, "play resumed");
    assert!(host.state.result().is_none(), "the ally fights on");
}

#[test]
fn the_host_hands_a_waited_seat_to_a_bot_and_its_client_hears_why() {
    use oxide_sim::scenario::{BotDifficulty, BotStance};
    let mut lan = Match::start(with_bots());
    lan.run(3);
    // The client stops polling until the host waits for it.
    wait(|| {
        lan.step_host();
        (!lan.host.1.status(lan.now).waiting.is_empty()).then_some(())
    });
    let Match {
        mut now,
        host: (mut host, mut link),
        client: (mut client, mut client_link),
    } = lan;
    link.replace_seat(
        &mut host,
        CLIENT,
        BotDifficulty::Standard,
        BotStance::Balanced,
    )
    .unwrap();
    assert!(host.scenario.players[usize::from(CLIENT.0)].bot);
    assert!(link.status(now).waiting.is_empty());
    let end = wait(|| {
        now += STEP;
        thread::sleep(Duration::from_millis(1));
        assert_eq!(link.pump(&mut host, now), None);
        client_link.pump(&mut client, now)
    });
    assert_eq!(end, End::Replaced);
    let resumed = host.state.current_tick();
    for _ in 0..5 {
        now += STEP;
        assert_eq!(link.pump(&mut host, now), None);
    }
    assert!(host.state.current_tick() > resumed, "play resumed");
    assert!(host.state.accepts_commands(CLIENT), "nobody surrendered");
}

/// Cuts the host's connection to the client, as a crash or a network drop
/// would, without telling the host's session.
fn cut(lan: &mut Match) {
    let Link::Host(host) = &mut lan.host.1 else {
        unreachable!("the host hosts");
    };
    host.peers.clear();
}

#[test]
fn a_client_that_loses_its_connection_rejoins_and_stays_in_sync() {
    let mut lan = Match::start(with_bots());
    lan.run(20);
    cut(&mut lan);
    // An order given while reconnecting still reaches the match.
    let order = Command::Stop { units: Vec::new() };
    wait(|| {
        assert_eq!(lan.step(), (None, None));
        (lan.client.0.presentation.net_notice.as_deref() == Some("Reconnecting to the host..."))
            .then_some(())
    });
    lan.client.0.issue(order.clone());
    let mut rejoined = false;
    wait(|| {
        assert_eq!(lan.step(), (None, None));
        if let Some(game) = lan.client.1.take_rejoined() {
            lan.client.0 = *game;
            rejoined = true;
        }
        (rejoined && lan.host.1.status(lan.now).waiting.is_empty()).then_some(())
    });
    assert_eq!(lan.client.0.presentation.human, CLIENT);
    lan.run(20);
    lan.drain();
    assert_eq!(lan.client.0.state.hash(), lan.host.0.state.hash());
    let sent = PlayerCommand {
        player: CLIENT,
        command: order,
    };
    assert!(
        lan.host
            .0
            .recorder
            .commands
            .iter()
            .any(|timed| timed.command == sent),
        "the order given while reconnecting executed"
    );
    assert!(
        lan.host.0.state.accepts_commands(CLIENT),
        "nobody surrendered"
    );
}

#[test]
fn a_rejoin_after_the_host_dropped_the_seat_hears_it_is_gone() {
    let mut lan = Match::start(with_bots());
    lan.run(5);
    cut(&mut lan);
    lan.host.1.drop_seat(CLIENT);
    let end = wait(|| {
        let (host, client) = lan.step();
        assert_eq!(host, None);
        client
    });
    assert_eq!(end, End::SeatGone);
}

#[test]
fn a_quiet_client_shows_on_the_host_while_play_runs_on() {
    let mut lan = Match::start(duel());
    lan.run(3);
    // The client stops polling, so it falls silent once its last lines land.
    wait(|| {
        lan.step_host();
        (lan.host.0.presentation.net_notice.as_deref() == Some("Lost connection to Alex"))
            .then_some(())
    });
    assert!(lan.host.1.status(lan.now).waiting.is_empty());
    wait(|| {
        assert_eq!(lan.step(), (None, None));
        lan.host.0.presentation.net_notice.is_none().then_some(())
    });
}

#[test]
fn a_client_hears_the_host_paused() {
    let mut lan = Match::start(duel());
    lan.run(3);
    lan.host.0.clock.paused = true;
    wait(|| {
        assert_eq!(lan.step(), (None, None));
        (lan.client.0.presentation.net_notice.as_deref() == Some("Paused by the host"))
            .then_some(())
    });
    for _ in 0..20 {
        assert_eq!(lan.step(), (None, None));
    }
    assert!(
        lan.client
            .0
            .presentation
            .toasts
            .iter()
            .all(|toast| !toast.text.contains("Waiting for the host")),
        "the notice explains the stall"
    );
}

#[test]
fn a_defeated_client_that_goes_away_holds_nothing() {
    let mut lan = Match::start(with_bots());
    lan.run(3);
    lan.client.0.issue(Command::Surrender);
    wait(|| {
        assert_eq!(lan.step(), (None, None));
        (!lan.host.0.state.accepts_commands(CLIENT)).then_some(())
    });
    let Match {
        mut now,
        host: (mut host, mut link),
        client,
    } = lan;
    drop(client);
    for _ in 0..40 {
        now += STEP;
        assert_eq!(link.pump(&mut host, now), None);
    }
    assert!(link.status(now).waiting.is_empty());
}

#[test]
fn a_client_loses_a_host_that_leaves_undecided() {
    let mut lan = Match::start(duel());
    lan.run(5);
    let Match {
        mut now,
        host,
        client: (mut client, mut link),
    } = lan;
    drop(host);
    let end = wait(|| {
        now += STEP;
        thread::sleep(Duration::from_millis(1));
        link.pump(&mut client, now)
    });
    assert_eq!(end, End::HostLost);
}

#[test]
fn a_decided_match_ends_cleanly_on_both_sides() {
    let mut lan = Match::start(duel());
    lan.run(3);
    lan.client.0.issue(Command::Surrender);
    wait(|| {
        assert_eq!(lan.step(), (None, None));
        matches!(
            &lan.client.1,
            Link::Client(ClientLink {
                connection: None,
                ..
            })
        )
        .then_some(())
    });
    assert!(lan.client.0.state.result().is_some());
    assert_eq!(lan.client.0.state.hash(), lan.host.0.state.hash());
    assert_eq!(recorded(&lan.client.0), recorded(&lan.host.0));
}

#[test]
fn a_host_leaving_a_decided_match_still_delivers_the_result() {
    let mut lan = Match::start(duel());
    lan.run(3);
    lan.host.0.issue(Command::Surrender);
    while lan.host.0.state.result().is_none() {
        lan.step_host();
    }
    let Match {
        mut now,
        host: (host, link),
        client: (mut client, mut client_link),
    } = lan;
    let mut lingering = link.leave(&host, now).expect("a decided host lingers");
    wait(|| {
        now += STEP;
        thread::sleep(Duration::from_millis(1));
        lingering.pump(now);
        assert_eq!(client_link.pump(&mut client, now), None, "no host loss");
        matches!(
            &client_link,
            Link::Client(ClientLink {
                connection: None,
                ..
            })
        )
        .then_some(())
    });
    assert_eq!(client.state.hash(), host.state.hash());
    wait(|| (!lingering.pump(now)).then_some(()));
}

#[test]
fn a_host_leaving_an_undecided_match_tells_its_clients() {
    let mut lan = Match::start(duel());
    lan.run(3);
    let Match {
        mut now,
        host: (host, link),
        client: (mut client, mut client_link),
    } = lan;
    let mut lingering = link.leave(&host, now).expect("the host says it left");
    let end = wait(|| {
        now += STEP;
        thread::sleep(Duration::from_millis(1));
        lingering.pump(now);
        client_link.pump(&mut client, now)
    });
    assert_eq!(end, End::HostLeft);
}

#[test]
fn a_client_leaving_surrenders_its_seat_and_the_match_plays_on() {
    let Match {
        mut now,
        host: (mut host, mut link),
        client: (client, client_link),
    } = Match::start(with_bots());
    let mut lingering = client_link
        .leave(&client, now)
        .expect("the client says it left");
    wait(|| {
        now += STEP;
        thread::sleep(Duration::from_millis(1));
        lingering.pump(now);
        assert_eq!(link.pump(&mut host, now), None);
        (!host.state.accepts_commands(CLIENT)).then_some(())
    });
    assert!(host.state.result().is_none(), "the ally fights on");
    assert!(
        host.presentation
            .toasts
            .iter()
            .any(|toast| toast.text.contains("left the match"))
    );
}

#[test]
fn an_injected_desync_halts_both_machines() {
    let mut lan = Match::start(duel());
    lan.run(5);
    lan.client.0.state.tick(&[]);
    let desync = End::Desync {
        tick: oxide_net::HASH_INTERVAL,
    };
    assert_eq!(lan.ends(), (desync, desync));
    assert!(desync.notice().contains("Desync at tick 20"));
    assert_eq!(End::HostLost.notice(), "Lost connection to the host.");
}

#[test]
fn a_desync_before_the_decisive_tick_still_halts_both_machines() {
    let mut lan = Match::start(duel());
    lan.run(5);
    lan.client.0.state.tick(&[]);
    // The host runs ahead and ends the match before the client
    // reports the diverged tick.
    while lan.host.0.state.current_tick() < oxide_net::HASH_INTERVAL {
        lan.step_host();
    }
    lan.host.0.issue(Command::Surrender);
    while lan.host.0.state.result().is_none() {
        lan.step_host();
    }
    let desync = End::Desync {
        tick: oxide_net::HASH_INTERVAL,
    };
    assert_eq!(lan.ends(), (desync, desync));
}

#[test]
fn a_mismatched_build_is_refused_and_its_seat_stays_open() {
    let mut host = host_lobby(duel());
    let mut client = Lobby::Client(ClientLobby::new(&address(&host), "other", name("Alex")));
    let reason = wait(|| {
        assert!(host.poll(Duration::ZERO, viewport()).is_none());
        assert!(client.poll(Duration::ZERO, viewport()).is_none());
        client.failure().map(|(_, _, reason)| reason.to_owned())
    });
    assert!(reason.contains("host runs build lan-test"));
    assert_eq!(client.status(), reason);
    let (joined_as, _, _) = client.failure().unwrap();
    assert_eq!(joined_as, &name("Alex"), "the form gets the name back");
    assert!(host.status().ends_with("1 of 2 players here"));
    assert!(host.failure().is_none());
}

#[test]
fn every_machine_sees_who_sits_where_and_a_leaver_frees_the_seat() {
    let mut host = host_lobby(three_humans());
    let mut client = Lobby::Client(ClientLobby::new(&address(&host), COMMIT, name("Alex")));
    wait(|| {
        assert!(host.poll(Duration::ZERO, viewport()).is_none());
        assert!(client.poll(Duration::ZERO, viewport()).is_none());
        (!client.roster().is_empty()).then_some(())
    });
    let expected = [
        "1. Host (host)",
        "2. Alex (you)",
        "3. Open",
        "4. Standard / Balanced AI",
    ];
    assert_eq!(client.roster(), expected);
    assert_eq!(client.status(), "Waiting for Host to start the match");
    assert_eq!(host.roster()[1], "2. Alex");
    assert!(host.status().ends_with("2 of 3 players here"));

    drop(client);
    wait(|| {
        assert!(host.poll(Duration::ZERO, viewport()).is_none());
        host.status().ends_with("1 of 3 players here").then_some(())
    });
    assert_eq!(host.roster()[1], "2. Open");
}

#[test]
fn a_joiner_that_falls_silent_loses_its_seat() {
    let mut host = host_lobby(three_humans());
    let mut client = Lobby::Client(ClientLobby::new(&address(&host), COMMIT, name("Alex")));
    wait(|| {
        assert!(host.poll(Duration::ZERO, viewport()).is_none());
        assert!(client.poll(Duration::ZERO, viewport()).is_none());
        host.status().ends_with("2 of 3 players here").then_some(())
    });
    // The client stops polling, so it stops heartbeating.
    assert!(host.poll(oxide_net::SILENCE_TIMEOUT, viewport()).is_none());
    assert!(host.status().ends_with("1 of 3 players here"));
}

#[test]
fn names_replace_the_seats_names_on_every_machine() {
    let lan = Match::start(with_bots());
    for game in [&lan.host.0, &lan.client.0] {
        let names: Vec<&str> = game
            .scenario
            .players
            .iter()
            .map(|player| player.name.as_str())
            .collect();
        assert_eq!(names[..2], ["Host", "Alex"]);
    }
    assert_eq!(lan.client.0.state.hash(), lan.host.0.state.hash());
}

#[test]
fn a_silent_connection_is_closed_after_the_greeting_timeout() {
    let Lobby::Host(mut host) = host_lobby(duel()) else {
        unreachable!("a host lobby");
    };
    let silent = std::net::TcpStream::connect(host.lobby.address()).unwrap();
    wait(|| {
        assert!(host.poll(Duration::ZERO, viewport()).is_none());
        (host.lobby.connection_count() > 0).then_some(())
    });
    assert!(host.poll(oxide_net::GREETING_TIMEOUT, viewport()).is_none());
    assert_eq!(host.lobby.connection_count(), 0);
    drop(silent);
}

#[test]
fn connections_past_the_greeting_cap_wait_to_be_accepted() {
    use std::io::BufRead as _;
    let Lobby::Host(mut host) = host_lobby(duel()) else {
        unreachable!("a host lobby");
    };
    let silent: Vec<_> = (0..MAX_GREETING)
        .map(|_| std::net::TcpStream::connect(host.lobby.address()).unwrap())
        .collect();
    wait(|| {
        assert!(host.poll(Duration::ZERO, viewport()).is_none());
        (host.lobby.connection_count() == MAX_GREETING).then_some(())
    });
    let waiting = std::net::TcpStream::connect(host.lobby.address()).unwrap();
    for _ in 0..10 {
        assert!(host.poll(Duration::ZERO, viewport()).is_none());
    }
    assert_eq!(host.lobby.connection_count(), MAX_GREETING);

    // The full slots time out, and only then is the waiting peer accepted
    // and greeted.
    let timeout = oxide_net::GREETING_TIMEOUT;
    assert!(host.poll(timeout, viewport()).is_none());
    wait(|| {
        assert!(host.poll(timeout, viewport()).is_none());
        (host.lobby.connection_count() == 1).then_some(())
    });
    waiting
        .set_read_timeout(Some(Duration::from_secs(5)))
        .unwrap();
    let mut hello = String::new();
    std::io::BufReader::new(&waiting)
        .read_line(&mut hello)
        .unwrap();
    assert_eq!(
        hello.trim_end(),
        oxide_net::LobbyMessage::hello(COMMIT).encode()
    );
    drop(silent);
}

#[test]
fn a_host_needs_its_own_seat_and_another_human_seat() {
    assert!(ScenarioHost::new("127.0.0.1:0", Scenario::skirmish(), HOST, COMMIT, None).is_err());
    assert!(ScenarioHost::new("127.0.0.1:0", duel(), PlayerId(2), COMMIT, None).is_err());
}

#[test]
fn a_host_can_sit_in_any_human_seat() {
    let mut lan = Match::start_as(duel(), CLIENT);
    assert_eq!(lan.host.0.presentation.human, CLIENT);
    assert_eq!(lan.client.0.presentation.human, HOST);
    lan.run(5);
    lan.drain();
    assert_eq!(lan.client.0.state.hash(), lan.host.0.state.hash());
}

#[test]
fn a_typed_address_gets_the_default_port_when_it_names_none() {
    for (typed, full) in [
        ("192.168.1.20", "192.168.1.20:4200"),
        (" connor-mbp ", "connor-mbp:4200"),
        ("connor-mbp.local.", "connor-mbp.local.:4200"),
        ("connor-mbp:5000", "connor-mbp:5000"),
        ("10.0.0.2:4201", "10.0.0.2:4201"),
        ("fd7a::1", "[fd7a::1]:4200"),
        ("[fd7a::1]", "[fd7a::1]:4200"),
        ("[fd7a::1]:4201", "[fd7a::1]:4201"),
    ] {
        assert_eq!(parse_join_address(typed).as_deref(), Ok(full), "{typed}");
    }
    let longest = format!(
        "{}.{}.{}.{}",
        "a".repeat(63),
        "b".repeat(63),
        "c".repeat(63),
        "d".repeat(61)
    );
    assert_eq!(parse_join_address(&longest), Ok(format!("{longest}:4200")));
}

#[test]
fn a_typed_address_that_cannot_reach_a_host_says_why() {
    for (typed, why) in [
        ("", "enter the host's address"),
        ("10.0.0.2:0", "the port must be a number from 1 to 65535"),
        ("host:99999", "the port must be a number from 1 to 65535"),
        ("host:", "the port must be a number from 1 to 65535"),
        (
            "192.168.1",
            "that isn't an address - try one like 192.168.1.20",
        ),
        (
            "192.168.1.300",
            "that isn't an address - try one like 192.168.1.20",
        ),
        ("-host", "that isn't an address - try one like 192.168.1.20"),
        ("a..b", "that isn't an address - try one like 192.168.1.20"),
        (
            "[fd7a::1",
            "that isn't an address - try one like 192.168.1.20",
        ),
        (
            "[nope]",
            "that isn't an address - try one like 192.168.1.20",
        ),
        ("a:b:c", "that isn't an address - try one like 192.168.1.20"),
    ] {
        assert_eq!(parse_join_address(typed), Err(why), "{typed}");
    }
    assert!(
        parse_join_address(&"a".repeat(64)).is_err(),
        "a label caps at 63"
    );
    let too_long = vec!["a".repeat(63); 4].join(".");
    assert!(parse_join_address(&too_long).is_err(), "a name caps at 253");
}

#[test]
fn an_unreachable_host_on_apple_points_at_local_network_permission() {
    let blocked = io::Error::from(io::ErrorKind::HostUnreachable);
    assert!(unreachable("10.0.0.2:4200", &blocked, true).contains("Local Network"));
    assert!(!unreachable("10.0.0.2:4200", &blocked, false).contains("Local Network"));
    let refused = io::Error::from(io::ErrorKind::ConnectionRefused);
    let reason = unreachable("10.0.0.2:4200", &refused, true);
    assert!(
        reason.starts_with("Cannot reach 10.0.0.2:4200: "),
        "{reason}"
    );
    assert!(!reason.contains("Local Network"), "{reason}");
}

#[test]
fn every_address_character_is_one_a_host_can_contain() {
    assert!("Ab9.-:[]".chars().all(address_char));
    assert!(!" /@_%".chars().any(address_char));
}

#[test]
fn a_wildcard_bind_shows_a_reachable_address() {
    let loopback: SocketAddr = "127.0.0.1:4300".parse().unwrap();
    assert_eq!(reachable(loopback), "127.0.0.1:4300");
    let shown = reachable("0.0.0.0:4300".parse().unwrap());
    assert!(
        shown.ends_with("4300") && !shown.starts_with("0.0.0.0"),
        "{shown}"
    );
}

#[test]
fn a_hosted_lobby_announces_itself_with_its_room() {
    let Lobby::Host(mut host) = host_lobby(three_humans()) else {
        unreachable!("a host lobby");
    };
    let mut scanner = oxide_net::Scanner::bind_to("127.0.0.1:0", COMMIT).unwrap();
    host.lobby.advertise_to(scanner.local_addr().unwrap());
    let port: u16 = host
        .lobby
        .address()
        .rsplit_once(':')
        .unwrap()
        .1
        .parse()
        .unwrap();
    let beacon = wait(|| {
        assert!(host.poll(Duration::ZERO, viewport()).is_none());
        scanner.poll(Duration::ZERO).first().cloned()
    });
    let oxide_net::discovery::Heard::Joinable(beacon) = beacon.heard else {
        panic!("this build's beacon");
    };
    assert_eq!(beacon.host, name("Host"));
    assert_eq!(beacon.port, port);
    assert_eq!(beacon.map, "Twin Forges");
    assert_eq!((beacon.players, beacon.seats), (1, 4));
}

#[test]
fn only_an_ipv4_listener_other_machines_can_reach_advertises() {
    for (bound, advertised) in [
        ("0.0.0.0:4200", true),
        ("192.168.1.20:4200", true),
        ("127.0.0.1:4200", false),
        ("[::]:4200", false),
        ("[fe80::1]:4200", false),
    ] {
        assert_eq!(advertises(bound.parse().unwrap()), advertised, "{bound}");
    }
}

/// A client link to a host this test plays by hand, over loopback.
fn hand_hosted(scenario: Scenario) -> (Game, Link, Connection) {
    let listener = Listener::bind("127.0.0.1:0").unwrap();
    let client = Connection::connect(listener.local_addr().unwrap()).unwrap();
    let host = wait(|| listener.try_accept().unwrap());
    let game = Game::networked(scenario, CLIENT, NetRole::Client, viewport()).unwrap();
    let home = Home {
        address: "127.0.0.1:9".to_owned(),
        commit: COMMIT.to_owned(),
        token: Token::new(1),
    };
    let link = Link::client(
        ClientSession::new(Duration::ZERO),
        client,
        home,
        Duration::ZERO,
    );
    (game, link, host)
}

#[test]
fn a_host_naming_a_seat_the_match_lacks_breaks_the_protocol() {
    let seats = oxide_net::HostMessage::Seats {
        seats: vec![SeatNotice {
            seat: PlayerId(9),
            state: SeatState::Quiet,
        }],
        paused: false,
    };
    let replaced = oxide_net::HostMessage::Replaced {
        seat: PlayerId(9),
        config: BotConfig::default(),
    };
    for line in [seats.encode(), replaced.encode()] {
        let (mut game, mut link, host) = hand_hosted(duel());
        host.send(&line);
        let mut now = Duration::ZERO;
        let end = wait(|| {
            now += STEP;
            link.pump(&mut game, now)
        });
        assert_eq!(end, End::HostLost, "{line}");
    }
}

#[test]
fn time_the_app_slept_does_not_count_against_the_rejoin_window() {
    let home = Home {
        address: "127.0.0.1:9".to_owned(),
        commit: COMMIT.to_owned(),
        token: Token::new(1),
    };
    let mut rejoin = Rejoin::start(home, Duration::ZERO);
    assert!(matches!(rejoin.poll(Duration::ZERO), RejoinPoll::Trying));
    // A minute asleep, then the window runs as frames do.
    let mut now = Duration::from_secs(60);
    assert!(matches!(rejoin.poll(now), RejoinPoll::Trying));
    while matches!(rejoin.poll(now), RejoinPoll::Trying) {
        now += STEP;
        assert!(now < Duration::from_secs(120), "the window never closed");
    }
    assert!(now >= Duration::from_secs(60) + REJOIN_WINDOW);
}
