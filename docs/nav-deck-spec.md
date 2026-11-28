# Nav-Deck Spec: the Flight Instrument

**Author:** vibe · **Date:** 26 Nov 2026 · **Status:** rev 1, written at claude's and the owner's ask
**Covers:** `desktop/navigation-deck` (#125, merged), `desktop/navigation-deck-chat` (the launcher), the `10 COCKPIT-CHAT` panel (#129, open at time of writing), NAV-TIME by devin (#121)
**Note on the name:** the owner has renamed the deck **nav-deck**; the file and class rename lands as its own PR after #129 — labels only per #123's numbering (see "Launch and placement" below).

## Purpose

comms-deck carries the conversation; nav-deck carries the map. One full-screen picture, refreshed about once a second, that answers from a sofa distance:

1. **Who is ready, busy, spent** — and who has notes waiting on them.
2. **Is the machine healthy** — memory above everything, because the deck was born the same morning as `Free swap = 0kB` in the kernel's OOM dump.
3. **Are the decks in step** — NAV-TIME compares each surface's own claim of freshness against the log's write lag, so a fast or slow clock shows as skew and a silent-but-full inbox shows as a stall.
4. **Where is the traffic heading** — the cockpit makes it visible at a glance without reading anything.

## The prime law

**Nav-deck is a viewer.** It never sends a note, never types into a window, and touches the deck in exactly one place: `w <seat>`, which invokes `comms-deck wake` so *that* program's own rules (never wake a spent agent) decide. Every safety property the deck has is inherited, not reimplemented.

## The screen

The layout follows the comms-deck look (claude's framework, *imported from the installed script, never copied*): strip on top, numbered panels in three columns, ticker and key hints on the bottom.

### Left column — the crew

- **01 CREW** — the ready order from `comms-deck status --json`, drawn with comms-deck's own builders so panel and status CLI cannot disagree.
- **02 ACTIVITY** — what each seat is doing, sheds its ↳ lines first when space runs short.

### Centre column — the cockpit (04 WARP CORE + 05 EVENT TAIL)

- **04 WARP CORE** — a first-person starfield: the view from the cockpit of a ship whose speed *is* the deck's traffic (notes this hour, busy seats, load). The **reticle — the sight at the vanishing point — sits on the screen's exact centre**, asserted by the self-test at pixel parity, because the owner asked for a heads-up display: "the reticle at the centre of the screen." Seats with waiting notes are plotted as **contacts**: their initial, at a bearing of their own, drifting with the field. WARP/SHLD readouts; the shield blinks when memory is low.
- **05 EVENT TAIL** — the cockpit's log under the core: seat state flips from `activity.jsonl` and the newest notes from `log.jsonl` (text redacted through the deck's own redactor, like the chat ticker).

### Right column — the instruments

- **03 USAGE** — per-seat plan usage (5-hour, weekly, monthly, context) from the statuslines' files.
- **06 WORKSPACES** — which windows sit on which workspace, real names where kitty exposes them.
- **07 VITALS** — memory left, an ~8-minute sparkline, the swap warning; carries the **stream status row** (obs/ffmpeg seen on the box).
- **08 NAV-TIME** (devin) — per-surface freshness, note→inbox latency (med/max over the newest 12), clock-skew warning, and a stall warning when notes wait while the log is silent.
- **09 DECK PULSE** — notes per hour today, and who carried them.
- **10 COCKPIT-CHAT** (#129) — the Twitch channel in the cockpit: newest last, `◆` broadcaster / `＋` mod chips, drawn from `~/.local/share/comms-deck/twitch-chat.jsonl` (devin's #130 daemon tails the IRC connection; the panel only ever tails the file — three independent daemons never touch one socket).

### Ticker and keys

Notes scroll along the bottom; `c` chat, `n` notes, `m` mail, `` ` `` comms deck, `2` mail deck, `1` back here. Nav-deck sends nothing — the keys move *you*, not a message.

## Data sources (all read-only)

`comms-deck status --json` · `log.jsonl` · `activity.jsonl` · `~/.local/share/comms-deck/` usage statuslines and inboxes · Hyprland `clients`/`workspaces` via `hyprctl -j` · `~/.local/share/comms-deck/twitch-chat.jsonl` (tail only, newest 32 KiB) · `twitch-chat.status.json` for the ● LIVE / ⚠ AUTH / ◌ connecting / ○ config state chip.

Nav-deck keeps its own history in `~/.local/share/comms-deck/navigation/` — one small file per day, nothing else written anywhere.

## Timing budget

The beat is about **1 second**; animation clocks are capped so the deck stays cheap on an 8 GB box with 9 agents: plain views ≤ 30 fps, the look's dashboards ≤ 5 fps (claude's #131 — a dashboard past 5 fps redraws forever and teaches nothing), the cockpit on its own beat. A `changed` signal still redraws instantly, so typing latency is never traded for smoothness. The narrow-screen form (under 150 columns) keeps the plain panels — nothing is lost on a small window.

## Hostile input

Cockpit chat renders what strangers type, so it is treated like a terminal-attacker surface (claude's review on #129): `scrub()` strips ESC, OSC sequences, C1, BEL, zero-widths, direction overrides and BOM from user (capped at 32), text and badges — a viewer must not reach STOX's terminal. A bad time must never kill the frame: the stamp is computed inside a guard catching `TypeError`/`ValueError`/`OverflowError`/`OSError` (nan and 1e30 fall back to "now" — `fromtimestamp` is the dangerous half, not `float()`), and the self-test's hostile records assert all of it.

## Launch and placement

`navigation-deck-chat` starts the kitty window; since #123's numbered decks, Hyprland opens it on workspace 2, silent and fullscreen, with the desk's `default_name` carrying the label (claude flips STOX's `default_name` to "nav" after the rename PR merges, so the label and the running config never disagree). Per #123's naming law the label is the prefix before `-deck`; the **rename to nav-deck** keeps the workspace number and the "2 silent" rule exactly as they are — labels only — because named workspaces sort alphabetically and would scatter waybar's key order.

## CLI contract

```
nav-deck              live full-screen view
nav-deck --once       one frame to stdout, exit (works with --plain for pipes)
nav-deck --self-test  render a fake tree, assert the contracts, touch nothing real
```

`--self-test` is the PR gate: every PR touching the deck must keep it green, and reviewable claims about what the panel tolerates must appear in it.

## Non-goals

- **Streaming.** The pipeline is #124's; nav-deck only *reports* whether a stream is running. Nothing streams until the owner says go.
- **Sending.** No notes, no typing, no replies — see the prime law.
- **Being the clock.** NAV-TIME reports skew and stalls; it does not correct anything.

## Acceptance checks

1. *The cockpit centred:* at any terminal size the self-test's reticle lands on the exact centre line/column, and the starfield's speed visibly tracks a change in notes-per-hour.
2. *The OOM morning cannot repeat silently:* with memory sinking to the floor, SHLD blinks and VITALS shows the dive in the sparkline.
3. *A hostile channel cannot take the screen:* nan/1e30/OSC-52/`\x1b[2J` payloads render inert or fall back; the frame survives them all (asserted, not believed).
4. *A stuck seat is visible from the sofa:* a note waiting > 1 minute on a silent seat appears as a contact in the cockpit and a stall line in NAV-TIME within one beat.
5. *The deck moves nothing:* `--self-test` and a `--once` run leave every log, inbox and status file byte-identical.

Written by: vibe (2026-11-26)
Draws on: Devin's NAV-TIME (#121); Claude's #120/#131 reviews; the hand-built cockpit history.
