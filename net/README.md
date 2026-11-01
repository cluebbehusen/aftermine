# aftermine-net

`aftermine-net` carries host-scheduled lockstep multiplayer. Every machine runs
the full simulation; only commands and session messages cross the network. The
host decides which commands execute on which tick, and every machine executes
the same ordered batches.

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
- `StartBarrier` takes a frozen roster to the first tick: zero for a new match,
  the saved tick for a resumed one.
- `HostSession` stamps each arriving command for the next unsealed tick, orders
  a sealed tick's human commands, gates sealing on client progress, compares
  client hash reports, and watches client liveness.
- `ClientSession` gates execution on received batches, acknowledges every
  executed batch, and watches host liveness.
- `Desk` and `Visit` are the host's and a client's halves of a probe or a rejoin
  while the match runs; `snapshot` splits the opaque world snapshot a rejoin
  carries into lines and puts it back together.
- `tcp` provides `Listener` and `Connection`, which move lines between machines.
- `discovery` defines the beacon a hosting lobby announces and the list of
  lobbies a joining player has heard; `udp` provides the `Advertiser` and
  `Scanner` sockets that carry it.

The [netplay architecture](../docs/netplay-architecture.md) describes joining,
starting, rejoining, the session contract, transport and discovery.

## Development

Run commands from the workspace root:

```sh
cargo test -p aftermine-net --locked
cargo test -p aftermine-driver --test lockstep --locked
cargo clippy -p aftermine-net --all-targets --locked -- -D warnings
```
