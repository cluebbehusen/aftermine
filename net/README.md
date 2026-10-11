# oxide-net

`oxide-net` carries host-scheduled lockstep multiplayer. Every machine runs the
full simulation; only commands and session messages cross the network. The host
decides which commands execute on which tick, and every machine executes the
same ordered batches.

The session core and start barrier perform no I/O and read no clock. Callers
pass received lines and a monotonic `now`, and collect lines to send. The `tcp`
and `udp` modules are the crate's only I/O. The crate does not own bots,
recording, or `State` itself: hosts run their own batch path of staged human
commands, then bot commands, then record, then `State::tick`.

## Main pieces

- `message` defines the JSON-lines wire messages: `JoinMessage` and
  `LobbyMessage` before the match, `ClientMessage` and `HostMessage` after Go.
  Decoding rejects unknown tags and unknown fields on messages that carry data;
  a line that fails to decode is a protocol error.
- `PlayerName` is a trimmed name of at most 16 printable ASCII characters,
  checked when typed and when decoded.
- `Gathering` and `Guest` are the host's and a joining client's halves of the
  lobby before Go. Which member sits in which seat is the caller's policy; the
  lobby carries it.
- `StartBarrier` takes a frozen roster to tick zero.
- `HostSession` stamps each arriving command for the next unsealed tick, orders
  a sealed tick's human commands, gates sealing on client progress, compares
  client hash reports, and watches client liveness.
- `ClientSession` gates execution on received batches, acknowledges every
  executed batch, and watches host liveness.
- `tcp` provides `Listener` and `Connection`, which move lines between machines.
- `discovery` defines the beacon a hosting lobby announces and the list of
  lobbies a joining player has heard; `udp` provides the `Advertiser` and
  `Scanner` sockets that carry it.

## Joining and starting

- Each side opens with `Hello`, carrying `PROTOCOL_VERSION`, `SIM_VERSION` and
  the build commit, and hangs up if the other side's do not match. Hello's shape
  never changes, so mismatched builds can still explain the refusal. Before
  launch the versions stay fixed and the commit decides
  ([versioning](../docs/versioning.md)). Local changes on top of a matching
  commit are the players' responsibility.
- After a matching Hello the client sends `Introduce` with its name. The host
  seats the member or refuses it with `Full`, and may later drop it with
  `Removed`. Each seated member receives the map once in `Map`, then a `Room`
  whenever who sits where changes from its point of view. The map stays fixed
  while the lobby is open.
- Before Go both sides heartbeat after `HEARTBEAT_INTERVAL` without sending, and
  the host drops a member it has not heard from for `SILENCE_TIMEOUT`, one that
  has not introduced itself within `GREETING_TIMEOUT`, and one that sends a line
  longer than `MAX_JOIN_LINE_BYTES` or out of turn.
- The host freezes the roster, which must hold exactly the scenario's human
  seats, and `StartBarrier` sends each client its seat, the `Scenario`, and an
  attempt number. Each client builds the match and replies `Ready` with its
  tick-zero `State::hash` and the attempt. When every hash matches the host's,
  the barrier queues `Go` and returns the `HostSession`, whose liveness clocks
  start there. Clients start their `ClientSession` on `Go`.
- A mismatched hash, a stray line, a seated member leaving or being removed, or
  `START_TIMEOUT` fails the start. `Gathering` drops every member who caused it
  and shows the rest the room again, which tells their `Guest` the start is off;
  `Gathering::cancel_start` does the same for everyone at the host's request. A
  `Ready` for an earlier attempt is ignored, so it cannot count toward the next
  start. Introductions that complete during a start wait for its outcome.

## Session contract

- A batch for tick N turns state N into state N+1. Clients acknowledge each
  batch with the resulting tick and attach `State::hash` when that tick is a
  multiple of `HASH_INTERVAL`.
- Human seats still in play go first in a sealed batch, in an order that rotates
  each tick over those seats, so neither the host nor the lowest-latency client
  always wins same-tick conflicts. Each seat's commands keep arrival order. A
  seat that left seals its last orders and its `Surrender` behind them. Hosts
  append bot commands after the sealed humans.
- The host seals tick N only while N is less than every live client's
  acknowledged tick plus `LEAD_CAP`. A client the host has heard nothing from
  for `QUIET_AFTER` is quiet: play continues within `QUIET_LEAD_CAP` of it.
- A client silent for `SILENCE_TIMEOUT`, one that has held the host back at the
  lead cap for `PROGRESS_TIMEOUT` even while heartbeating, a closed connection,
  or a protocol violation makes the seat away: the host stops sealing and waits
  for its player, with no timeout and no automatic surrender. A connection that
  is heard again and catches back up to within `LEAD_CAP` ends the wait. Only
  the host ends it early: `drop_seat` surrenders the seat, and `replace_seat`
  hands it to a bot the host then runs, dropping the seat's unsealed orders and
  telling every client which bot took it. Either way the seat's client hears
  why. A client that sends `Leave` surrenders without holding the match, and a
  seat that can no longer act in the world retires, its client free to watch.
- The host tells every connected client which seats are quiet or away, and
  whether it paused, whenever that changes. A gap between a peer's own polls
  longer than `SUSPEND_GAP` counts as that peer's suspension, never as the other
  side's silence.
- A client line longer than `MAX_CLIENT_LINE_BYTES`, or orders that would leave
  more than `CLIENT_PENDING_BYTES` of a client's commands waiting, is a protocol
  violation. A sealed batch carries at most `BATCH_COMMAND_BYTES` of human
  commands; later orders wait for the next tick in arrival order, so a batch
  line stays under `MAX_LINE_BYTES`.
- A mismatched hash report halts the session and tells every client. The host
  keeps its hashes until every connected client has reported them, so a client
  catching up from a quiet spell still has them checked.
  `HostSession::caught_up` reports when every client the host is not waiting on
  has acknowledged every published batch; a host ending a decided match waits
  for it so the final reports are still checked. A host leaving an undecided
  match tells its clients so.
- Each side heartbeats when it has sent nothing for `HEARTBEAT_INTERVAL`.
  `HostSession::poll` must run on the host's game loop: a host whose loop hangs
  stops heartbeating, and clients end the session after `SILENCE_TIMEOUT`.
  Clients therefore need no progress timeout of their own.

## Transport

- Lines are newline-delimited and at most `MAX_LINE_BYTES`. An oversized or
  non-UTF-8 line shuts the connection down in both directions.
- Each `Connection` has a reader thread and a writer thread, so a slow peer
  never blocks the game loop that polls it. Received lines wait in a bounded
  queue; when the game loop falls behind, the reader stops reading and TCP
  pushes back on the peer. Outgoing lines are bounded by the session's lead
  caps, since an away seat stops sealing. A write that stalls past the write
  timeout or fails closes the connection. There is no read timeout: the lobby
  may wait indefinitely, and in-match liveness belongs to the session core.
- Dropping a `Connection` aborts it and discards unsent lines. To close cleanly,
  call `finish` and keep polling until `Closed` before dropping; after `finish`,
  `Closed` waits until every queued line has been written, so the final lines
  reach the peer.
- `Listener::try_accept` never blocks, so the game loop can poll it. Bind a
  non-loopback address for other machines to reach. `DEFAULT_PORT` is the
  conventional port.

## Discovery

- A hosting lobby sends a `Beacon` to UDP `DISCOVERY_PORT` once per
  `BEACON_INTERVAL`: its host's name, TCP port, map, how many players are here,
  and how many seats the map has. It goes to the limited broadcast address and
  to the broadcast address of the subnet the machine routes through, taken as a
  /24, since macOS can refuse the limited address when several interfaces are up
  and std cannot read a netmask. A joining player's `Scanner` lists what it
  hears, dropping a lobby unheard for `BEACON_EXPIRY`.
- A beacon opens with fields whose shape never changes (a tag, the protocol and
  sim versions, the commit, and the host's name), so a lobby of another build
  still lists, marked unjoinable.
- Broadcasts stay on the local network, so a routed network such as Tailscale
  needs a typed address. iOS sends and receives broadcasts only for apps with
  Apple's multicast entitlement; elsewhere a refused socket leaves discovery
  off.

## Development

Run commands from the workspace root:

```sh
cargo test -p oxide-net --locked
cargo test -p oxide-driver --test lockstep --locked
cargo clippy -p oxide-net --all-targets --locked -- -D warnings
```
