# Changelog

## Unreleased

- **ACTIVITY cooldowns in metric time.** The spent "idle until" countdown
  (under an hour) and the polling seats' "next check"/"check late" timers
  now show milliseconds — `58:20.250`, `1d 00:00:00.000` — and the screen
  redraws at 10 fps while one is live, so the digits actually move.
  Longer waits keep the coarse countdown. By Devin, on the owner's ask.
- **ACTIVITY back on the right.** The usage panel and the context pie
  grew tall enough to push the ready order off the screen. Wide screens
  now draw three columns — USAGE + CONTEXT SPLIT left of the notes,
  ACTIVITY on the right — and narrower ones stack ACTIVITY above USAGE
  in the single right column, so the ready order always shows. By Devin,
  on the owner's ask.
- **Fresh code after a merge.** The service, the chat screen and
  scratchpad-keeper restart themselves in place when their program file
  changes on disk. They wait 3 seconds for the write to finish and check
  the new file compiles first. The chat screen waits until its box is
  empty, so a half-typed line is never lost. An MCP server started before
  the change runs each tool call through the current program, so a new seat
  works without restarting the agent.
- **How the agents' context split, as a colour-coded pie.** The usage
  panel draws a text pie beside its legend: each seat's share of the live
  context its usage file reports, biggest slice at 12 o'clock, one total
  line naming the seats in it. Under it, one small bar per reporting seat,
  in the seat's colour, showing how full its own window is. Both pulse,
  holographic-style: the hue stays the key's, a brightness wave and a
  white glint ride the redraw clock. The panel's older usage bars (each
  seat's context, its 5-hour and weekly limits) pulse with the same
  recipe, keeping their green/amber/red rule. The activity words and the
  spent countdowns don't animate. Seats that
  report no context figure are left out rather than counted as zero. By
  Vibe, on the owner's ask.
- **Leave a seat out with `-seat`.** `all -grok` or, on the chat screen,
  `-grok hello` goes to everyone but Grok. Only real seat names count, so
  `re-run` and `-rf` stay as text. The record lists the real recipients.
  By Vibe (#49, reviewed by Claude).
- **Vibe's usage bar.** The usage panel shows Vibe's context: tokens in use
  out of the model's window, and the model's name, read every 30 seconds
  from the files Vibe writes for its live session. Vibe's free models
  report no plan limit, so there's no 5-hour or weekly bar. By Claude, on
  the owner's OK to read Vibe's files.
- **Devin joins as the `devin` seat.** It has hooks through Claude's path,
  the MCP server with `COMMS_DECK_SEAT=devin`, the deck's tools allowed in
  `~/.config/devin/config.json`, and an install step. Claude's hooks, which
  Devin also reads, stay quiet inside Devin. By Claude (#50, reviewed by Grok).
- **Scratchpad:** Vibe has the top-right cell and the small chat is gone
  (#45). Lumo, Gravity and Devin share the bottom right (#51). By Claude,
  reviewed by Vibe, Grok and Alice.
- **Codex's usage limit read off its screen**, with a countdown to the reset
  (#41). By Claude. Vibe found and reviewed the fix for an unreadable reset
  time that rewrote the mark every 30 seconds.
- **A late check drops to idle.** A seat that only polls (Lumo) shows idle,
  not stuck and not busy, once her expected check is late. Notes stay
  queued. By Grok (#43, reviewed by Vibe).
- **Recovery is written down.** [docs/REDUNDANCY.md](docs/REDUNDANCY.md)
  says what still works when a seat is down, how another agent brings it
  back, and how to recover the program and the pushed notes from GitHub
  when the machine is gone. The public cold-start manual is
  [navigation-control-system](https://github.com/whyyyyyai/navigation-control-system).
  No new program. Vendor chat transcripts are still not copied anywhere.
  By Grok (#25).

## quantum-kittens (2026-11-24)

The easy start guide is [quantum-kittens.md](quantum-kittens.md), by Grok,
with follow-ups by Claude (#35). Each entry names who wrote it.

- **Out of usage, with a countdown.** An agent whose plan limit is full
  shows a grey `○ idle until Tue 05:08 · 6d 21h 52m`, from its usage meter
  or from `comms-deck spent <seat> <time>` for agents with no meter. It
  clears itself at the reset. By Claude (#27), building on Grok's stuck row
  (#21) and spent row (#24).
- **ACTIVITY sorted by availability.** Ready, busy, stuck, away; then
  fewest notes waiting; idle until is always last. Idle now reads "ready". By Claude (#33).
- **One availability score** (`availability()`, `most_available()`) for the
  panel and for picking who gets a task, the base for the load balancer. A
  spent agent whose seat file still says idle is no longer picked. By
  Claude (#34).
- **Lumo's row**: busy or ready from her own tool calls, and a countdown to
  her next check. By Claude (#28).
- **App-seat delivery fixed.** The "did it land" check ignores whitespace,
  so a sent batch is no longer typed twice. The deck clears its own
  leftover text. A draft left untouched for 10 minutes is saved to the
  owner and cleared; the row says "draft in box" meanwhile. Replies go to
  the owner, not whichever sender sorts first. By Claude (#29, #31).
- **`@deck` desktop actions** from a fixed list; unlisted requests from app
  seats are refused. By Claude (#18). **`comms-deck review-check`**, by
  Grok (#23).
- **Chat views** full, half and small (#26), and **scratchpad-keeper**, a
  user service that keeps the scratchpad arranged at login and when windows
  change, leaving hand moves alone. By Claude (#26, #32).
- **Under each ACTIVITY row, six words on what is outstanding**: what the
  agent says it is doing while busy, otherwise the oldest note waiting
  (a private note shows only "private note"). No count; the blue squares
  count. The self-test no longer pops up desktop notifications. By Claude
  (#31, #35).

## comms-deck 2.0 (2026-11-24)

The first release under the name comms-deck (formerly launchpad). It
replaces launchpad 1's `relay-protocol`, `scheduler` and `relay-watch` with
one program, built for a Hyprland + kitty desktop with the repos in the home
folder.

### Messaging

- **A local mailbox.** Each note is one file in the recipient's inbox; taking
  it is an atomic rename, so it's delivered exactly once. No network involved.
- **MCP tools for every agent:** `send_message`, `read_messages`,
  `set_status`, `add_note`, `team_status`.
- **Delivery without polling or typing over anyone.** A working agent gets
  its notes from its own hooks (with the owner's next prompt, at the end of
  a turn, or for Gravity before each model call). An idle agent has the note
  pasted into its own kitty window through remote control, without moving
  focus, and never while there's unsent text in that window or it has focus.
- **Seats:** `claude` (Claude Code), `grok` (Grok CLI), `gravity`
  (Antigravity CLI), `codex`, app seats `alice` and `juno` (Grok Bot app),
  `lumo` (not set up), and `owner`. Terminal agents are recognised by their
  process.
- **Grok Bot app seats** (Grok). The service types notes into a bot's own
  chat through the app's debugging port (127.0.0.1 only) and files its
  replies as notes; `@seat` at the start of a reply addresses another agent.
  It never reads the app's cookies, storage or tokens.
- **Loop guard.** After 20 automatic hand-overs in a row with nothing from
  the owner, delivery to that agent pauses. Owner notes always get through.

### The chat screen

- Every note as it's sent, coloured by seat; the owner shows as "You".
- **USAGE:** context and limits for each agent (Claude's 5-hour and weekly
  limits, Grok's context and session totals read from its own session files,
  Gravity's model, the Grok Bot account's weekly limit shown once), with a
  live **~$ total** of the costs the agents report. Seats with no data are
  left out rather than shown empty.
- **ACTIVITY:** each change of state, with what the agent is working on
  (from what started its turn, or from `set_status`). Flicker under 3 s is
  hidden.
- **Status bar:** a light per seat, with its state, how long, and waiting
  notes; hidden seats are left off.
- **Chat box:** `@seat` (several allowed), `@all`, or no `@` for everyone.

### Privacy and safety

- Secret-looking notes are made private and never reach Discord or the
  activity log.
- Names in `~/.config/comms-deck/private-names` become "the owner" in every
  copy that leaves the machine.
- comms-deck opens no network port of its own.

### Setup

- `comms-deck install` sets up Claude Code, Grok, Antigravity and Codex
  (hooks, MCP server, status lines), kitty remote control, the services, the
  chat screen launcher, `scratchpad-layout`, an app menu entry and the
  Hyprland rule. It's safe to run again. For Antigravity it also allows the
  five comms-deck tools in agy's permission list, so they don't prompt on
  every call (Grok, PR #14).
- [docs/SETUP.md](docs/SETUP.md): a fresh Linux machine to a working team,
  step by step, with checks.

### Removed

- `relay-protocol`, `scheduler`, `relay-watch` and their services (in the
  history). The `relay-protocol` and `relay-public` repos are archived.

---

Written by: Claude, from the pull requests. Each entry names its author:
Grok, Vibe and Claude so far.
