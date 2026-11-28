# Retiring comms-deck: moving the project into the warp core

Y, 2026-11-27 05:04: the warp core is the default, and moving systems and
code over from comms-deck should end with the warp core as the standalone
program that runs the project. This is the order to do it in. Each step
ships on its own and leaves both programs working, because the deck must
never go dark mid-move (see "Prefer deterministic programs": nothing may
depend on an agent staying alive).

## Where things stand

The warp core (Rust and Bevy, `warp-core/`) is a **reader and a sender**.
Everything else is still done by `comms-deck` (Python, one file):

| The warp core uses | Through |
| --- | --- |
| seats and states | `comms-deck status --json`, polled |
| notes | tail of `~/.local/share/comms-deck/log.jsonl` |
| activity, notifications | `activity.jsonl`, `notifications.jsonl` |
| tasks | `tasks/<id>.json` |
| meters | `usage/<seat>.json` |
| sending as Y | `comms-deck send` |
| focusing a window | `hyprctl` |

comms-deck still owns the rest: the service loop, the hooks, kitty wake,
the Grok Bot page, the mailbox and delivery, records and the Discord
mirror, desktop actions, the load balancer, mail, the MCP server, install,
and the self-test.

## The order

1. **Stand on its own feet (now).** The warp core runs as a user service
   and is the default screen at login. It survives a bad value in the
   feed (one `Infinity` once emptied the whole ring). Crashes are filed
   in quantum-kittens.md.
2. **Read state directly.** Started: the service writes
   `~/.local/share/comms-deck/status.json` every 2 s and the warp core reads
   that file, running the command only when the file is stale (t28; this
   the warp core with its children went from 46% to about 29% of a core). Next: read `seats/<seat>.json` and the pid checks
   in Rust, instead of shelling out to `comms-deck status --json` every
   poll. This is the first logic to move. Keep the JSON shape as the
   contract, and test the warp core against comms-deck's self-test
   fixtures.
3. **Own the mailbox.** Write notes (`log.jsonl`, inboxes) from the warp
   core, so the chat panel no longer needs `comms-deck send`. Turn on the
   MCP tools (send, read, status, tasks) only once their output matches
   comms-deck's, byte for byte, in tests.
4. **Own delivery.** Hooks and kitty wake: the parts that hand an agent its
   notes. This is the riskiest step, so run both for a day with the warp
   core in shadow mode, logging what it would do, before switching over.
5. **Own the load balancer and the task board.**
6. **The rest:** records, the Discord mirror, desktop actions, mail. The
   nav deck comes down once the warp core is stable (Y, 05:17): its
   cockpit and meters are already in the warp core. comms-deck shrinks to `install` and `self-test`, then retires.
   The warp core gets a headless mode, so the logic runs even when no
   window can open.

## Rules for every step

- Build with `-j 2`. Run `cargo test` and `comms-deck self-test` before
  installing. Install with `warp-core/install.sh`.
- One owner per data file at a time. Never let both programs write the
  same file.
- A crash, or a step that has to be undone, goes in quantum-kittens.md.

Written by: claude
