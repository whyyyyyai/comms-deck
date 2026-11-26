# One deck in Rust: plan

**Asked by:** Y, 2026-11-26 16:16: "I want the comms-deck and navigation deck
to be integrated into a single program, written in rust, with a modern user
interface. You may ideate and iterate."

**Status:** rev 1, plan only. Written by: Claude.

## What changes

Today there are two Python screens, each in its own kitty window:

- `comms-deck chat` (workspace 1, comms): the conversation, the crew, activity, usage.
- `desktop/nav-deck` (workspace 2, nav): warp core, vitals, nav-time, workspaces, cockpit chat.

They become one Rust program, `deck`, with both as views of one screen:

| Key | View |
| --- | --- |
| `` ` `` / `F1` | **Comms**: the chat log, a compose box, crew and activity beside it |
| `1` / `F2` | **Nav**: warp core in the centre, instruments around it |
| `Tab` | the other view |
| mouse | click a seat to address it, scroll the log, click a panel to zoom |

"Modern" means: truecolor seat colours, rounded panels, a real text box
(cursor, history, multi-line paste), mouse, smooth animation where it
helps (the warp core) and none where it doesn't (≤ 5 fps dashboards, per #131).

## What does not change

The Python `comms-deck` stays the **service**: delivery, wake, hooks, MCP,
the live-tree guard, redaction, review-check. `deck` is a client of it:

- It **reads** the files the service already writes:
  `comms-deck status --json`, `log.jsonl`, `activity.jsonl`, `usage/*.json`,
  `navigation/`, `twitch-chat.jsonl`, and `hyprctl -j`.
- It **sends** only by running `comms-deck send` (and `comms-deck wake`),
  so every rule (never wake a spent seat, redaction, the loop guard) is
  inherited, not rewritten. This is nav-deck's prime law, kept.

So a bug in `deck` can make a screen look wrong; it can't lose a note or
wake a spent agent. Porting the service itself to Rust is a separate,
later decision for Y, and only once `deck` has replaced both screens.

## Shape

```
deck/                 Cargo crate in comms-deck (one binary)
  src/main.rs         args: (none) live · --once · --self-test
  src/data.rs         read status --json, the jsonl tails, usage, hyprctl
  src/send.rs         comms-deck send / wake, nothing else writes
  src/ui/comms.rs     chat log + compose + crew + activity
  src/ui/nav.rs       warp core + usage + vitals + nav-time + workspaces + cockpit chat
  src/ui/theme.rs     seat colours, borders, the look
  src/scrub.rs        strip ESC/OSC/C1/bidi from anything shown (same rules as Python's scrub)
```

Crates: `ratatui` + `crossterm` (TUI, mouse), `serde_json`. Nothing networked.

## Steps (each one a PR into test)

1. **Skeleton:** both views drawn from live data, read-only, `--once` and
   `--self-test` (ratatui's TestBackend, fake data tree, touches nothing real).
2. **Compose:** the text box, `@seat` addressing, send through `comms-deck send`.
3. **Nav parity:** warp core with Devin's seat-coloured stars spread by
   context split (ported from his nav-deck PR), nav-time, cockpit chat.
4. **Switch over:** `comms-deck-chat` and `nav-deck-chat` launch `deck`
   (`deck --view comms` / `--view nav`); the Python screens stay installed
   one week as the fallback, then Y decides.
5. `launch-codes` manifest + `launch.sh` build it (`cargo build --release`).

## Needs from Y

A Rust toolchain. Auto mode won't let an agent install one, so once, in a
terminal: `sudo pacman -S rustup && rustup default stable`.
