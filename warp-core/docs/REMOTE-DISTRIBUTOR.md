# Read-only Warp Core distribution

The distributor lets another computer render the Warp Core scene locally. It
publishes the display model, not pixels and not the comms-deck state directory.
The receiver runs the normal Bevy renderer and has no path back to the deck.

## Trust boundary

- The publisher binds only to `127.0.0.1` by default. A remote viewer reaches it
  through an authenticated SSH local-forward (or an equivalent encrypted,
  authenticated tunnel). Do not expose the plain HTTP listener on a LAN or the
  public internet.
- The transport is one-way: `GET /v1/snapshot` and `GET /v1/events` only.
  Reject every other method. There is no command, chat-send, task, focus,
  filesystem, or configuration endpoint.
- Publish a purpose-built display DTO. Never serve files from the data,
  configuration, log, inbox, task, usage, or price directories.
- Omit hidden seats and private notes. Redact private task titles. Never include
  prompts, transcripts, environment variables, local paths, credentials, or raw
  hook payloads. Public chat text is included only after the same parser and
  scrubber used by the local Warp Core.
- Apply hard limits to snapshot size and collection lengths. A malformed or
  unsupported protocol version fails closed; a client must not fall back to
  opening local controls or sending commands.

The SSH tunnel is the authentication and encryption boundary. A bearer token
sent over an unencrypted remote HTTP connection is not an acceptable substitute.
The local listener should have a small connection limit and no directory or
file-serving handler.

## Wire protocol v1

`GET /v1/snapshot` returns one UTF-8 JSON object with `version: 1`, a monotonic
`sequence`, and the latest bounded display state:

```json
{
  "version": 1,
  "sequence": 42,
  "time": 1795842000.0,
  "seats": [],
  "tasks": [],
  "subagents": [],
  "activity": [],
  "notices": [],
  "messages": [],
  "prices": {}
}
```

Each collection contains only fields already needed by the renderer. In
particular, a message may contain its sanitized public display text, sender,
recipient and timestamp, but never its source log record or private flag. A
private task has its title set to null. Seat usage values are numeric display
metrics, not the source usage file. Price entries contain symbol, price and
bounded history; no local file information.

`GET /v1/events?after=<sequence>` is a Server-Sent Events stream. Each `data:`
record is a complete versioned state update with the next sequence. If a client
reconnects after the retained event window, the server sends a `resync` event;
the client fetches a fresh snapshot. The publisher bounds retained events and
disconnects slow consumers instead of allowing unbounded memory growth.

## Client behavior

The viewer accepts a stream URL only on loopback. To view a remote publisher,
the operator first opens an SSH forward, for example:

```sh
ssh -N -L 48765:127.0.0.1:48765 user@warp-core-host
```

Then the viewer connects to `http://127.0.0.1:48765`. It parses only v1 DTOs,
updates the same read-only scene state used by the local feed, and never invokes
deck commands. If the stream disconnects, it shows the last state with a stale
indicator; it does not enable local interaction or replay old activity as new.

## Implementation boundary

The publisher belongs beside Warp Core's feed reader, where private notes and
hidden seats are already filtered. Its shared state should be the sanitized DTO
after parsing, not raw files. The client adapter should translate DTO updates
into the existing `Msg`/scene state path. Keep the listener opt-in and its
bind address fixed to loopback in v1. This keeps the server small and means
remote rendering needs neither GPU capture nor access to the host's files.
