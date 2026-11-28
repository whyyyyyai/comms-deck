# Changelog

## Unreleased
- **A remote seat state reaches 02 ACTIVITY by beacon.** A remote
  seat (docs/REMOTE.md) publishes a small status.json from its own machine;
  the deck lists its url in ~/.config/comms-deck/remote-beacons, fetches it
  in the sync pass at a 30 s timeout, and applies it through update_seat()
  - so mystra ready-order row is a real state, not a permanent away.
  Known words only, the doing line passes the redactor, and a stale beacon
  pins away (a silent seat is an away seat). Hermetic self-test, stubbed
  urlopen. By Mystra.

- **A seat that comes online is told who's down, so bots help bots get
  back.** The service now watches the roster every 15 seconds (the owner's
  t64, 2026-11-27). When a seat starts answering notes - a live agent
  process, or an app seat whose connector is up - the deck sends it one
  note naming the seats that are down and their states, and asks it to
  help bring them back and report what it tried. Seats already online when
  the service starts aren't greeted, spent seats can't be greeted or named
  (a limit reset isn't something a bot can hurry), remote seats are never
  named (nothing on this desktop reaches them), and a seat is greeted at
  most once per `WELCOME_EVERY` seconds (default 6 h) so a flapping window
  can't nag. Self-test covers the seed pass, the greeting, the cooldown
  and the spent/remote exclusions. By Devin.

- **The owner's name is Y.** STOX renamed themselves (2026-11-26): screens,
  notes, the chat prompt (`y ›`) and Discord now show **Y**, and `y` names
  the owner wherever a seat is named. The seat id stays `owner` in files
  and logs, and the owner's earlier name `stox` is still accepted as the
  seat, old notes addressed `@STOX` included. Self-test asserts both
  spellings and that `( Y )` sits in the crew panel. By vibe.

- **navigation-deck is now nav-deck.** STOX renamed the deck (2026-11-26):
  the program, launcher and kitty class become `nav-deck` / `nav-deck-chat`
  (+ `nav-deck-half`), the desk label becomes **nav** (workspace 2 stays 2 —
  numbered decks per #123, so waybar's key order never changes), and
  `install()` writes the `^nav-deck$` window rule and the new default_name,
  removing a previous install's navigation-deck links and app entry. The
  data dir `~/.local/share/comms-deck/navigation/` keeps its name: history
  continuity beats cosmetic consistency. Claude flips the running hyprland
  `default_name` after this merges so the label and the config never
  disagree. By vibe.

- **navigation-deck: the desktop's flight instrument, on the navigation
  deck (Command+1).** A
  viewer born of the 02:11 OOM: the deck's ready order with note loads and
  spent countdowns, per-seat plan meters, the workspace map (kitty windows
  get their seat's name by walking the window's process tree), memory
  vitals with a live sparkline and a swap warning, and the day's note
  pulse. At the owner's request the middle column is the **warp core**: a
  first-person cockpit whose starfield speed is the deck's own traffic, with
  seats with waiting notes plotted as contacts around the reticle, and
  WARP/SHLD readouts (notes/hour, busy seats and load vs memory free).
  Reads `comms-deck status --json` and the deck's own logs; sends
  nothing; its `w <seat>` key goes through `comms-deck wake`, so the deck's
  own rules decide. `navigation-deck-chat` opens it fullscreen on the
  navigation deck (workspace "navigation", Command+1). By vibe.

- **Firefox gets the whole left half while Grok is offline.** Claude moves
  from the bottom-left cell into the bottom-right row, after Devin (STOX,
  2026-11-26). With Grok back, the old layout returns. By Claude.
- **The owner is STOX.** The owner chose a name (2026-11-26): screens,
  delivered notes and Discord show STOX instead of "You" or "owner", the
  crew panel's centre reads ( STOX ), the chat box prompt is `stox ›`, and
  agents are told to call them STOX. The seat id stays `owner` in files and
  logs; `stox` is accepted wherever a seat is named. By Claude.
- **02 and 03 swap places in the look.** 02 ACTIVITY now sits under 01 CREW
  on the left, and 03 USAGE heads the right column above 05 and 06 (the
  owner, 2026-11-26). 03 takes the rows it needs; 06 gets the rest. By
  Claude.
- **comms-deck mail: the owner's mailbox, on workspace 2.** A small mail
  client in the chat screen's style — inbox list, read pane, compose with
  reply — over Proton Mail Bridge on localhost (IMAP 1143 / SMTP 1025 by
  default). The Bridge password stays in ~/.config/comms-deck/mail.secret
  (chmod 600), mail.json holds the address and ports, and the client only
  reads (BODY.PEEK), so opening a message never marks it read on the
  server; it re-checks the inbox every minute while open, so no agent has
  to stay up. The desktop half (launcher, app entry, the workspace-2 window
  rule) is Vibe's #107. By Devin.
- **The chat stays fullscreen on workspace 1; Firefox in for an offline
  Grok.** scratchpad-layout --watch puts the full chat view back in
  fullscreen (and on workspace 1) whenever Hyprland takes it out, e.g. when
  another window opens there. While Grok is offline, Firefox fills the
  scratchpad's top-left cell and Grok Bot is minimised (special:minimised);
  when Grok is back, Grok Bot returns and Firefox goes to workspace 3. By
  Claude.
- **The look's right column at half height.** With the chat on the top
  half of the screen and a terminal below, 02 ACTIVITY was taller than the
  column, so it was skipped and 06 with it, leaving the column blank. Short
  of room, 02 now drops its `↳` lines (one row per seat); 06 shows when it
  fits. By Claude.
- **Gravity / Antigravity.** An easter egg from the owner: Gravity is named
  Antigravity while it's online, in 01's crew labels and 02's rows. By Claude.
- **The mail screen's window rule (workspace 2).** For the mail client
  (Devin builds the view): `comms-deck install` now also links a
  `comms-deck-mail` launcher (kitty, class `comms-deck-mail`), adds its app
  menu entry, and, for a Lua Hyprland config, a rule putting it fullscreen
  on workspace 2 — Super+2 already goes there, so no new keybind (Claude).
  `comms-deck mail` itself is still to come. By Vibe.
- **Telemetry records can name provider, account and models.** A usage
  feed may now declare `data.provider`, `data.account`/`account_id` and
  `data.models`; the seat/source labels stay the fallback, so a remote
  app seat reads provider `unknown` instead of the transport `remote`,
  and two accounts on one provider can stay distinct. Account pooling is
  provider-scoped, so the same account id under two providers never folds
  into one row. By Devin, from the 03 review.
- **The cost ledger, hardened (Vibe's review of #102).** A seat without
  session ids now keys its generations by machine as well (`seat|machine|gen-N`).
  Two machines' counters both start at 0, so a restored ledger can no longer
  merge different sessions. A drop in cost opens a new session only when the
  next reading is low too. One flaky low figure is held and then dropped
  when the cost comes straight back, so the climb back isn't counted twice.
  The docs say a pool's all-time figure is the sum of its seats', and that
  only a machine wiped within the ten-minute push window loses anything.
  By Claude.
- **One Grok bot.** The owner (23:08): the Grok Bot app runs one bot now,
  so the second bot seat is gone from the code, the docs and the setup
  examples; the pool comments in the telemetry layer describe the bot seats
  generally. Released CHANGELOG entries are corrected in place, with the
  history in git. By Vibe.
- **All-time cost in the top bar, kept in a ledger.** The owner (23:08):
  COST should be the all-time total and survive a reset or an upgrade.
  `DATA/cost-ledger.json` holds one entry per (seat, session) at the
  highest cost that session reported. It's keyed by the feed's session_id,
  or by a generation that moves on when a seat's cost drops. Two Claude
  sessions writing in turn therefore never undercount, and entries only
  go up. The status line folds a cost in as it arrives, and the service
  sweeps every seat each 30 s. It's mirrored to the private relay repo
  (`ledger/costs.json`) and merged both ways, so a fresh clone restores
  it. The top bar reads "$N · est. since <date>", and `usage <seat>`
  shows each seat's all-time figure as `cumulative`. Design reviewed with
  Vibe. By Claude.
- **Vibe's usage bar in 03.** The owner (23:01): Vibe's app shows a usage
  bar (52%), so 03 should too. Vibe fetches the figure from Mistral and
  only draws it, so `vibe_screen_gauge()` reads the % off the footer of
  Vibe's kitty window (the last three lines only, like the usage-limit
  check does for Codex). It's stored as `rate_limits.monthly` with no
  reset time, and 03 draws it as an `mo` bar under VIBE. An unreadable
  screen keeps the last reading. By Claude.
- **03: no token block, and Grok is one pool.** The owner (22:53): the
  TOKENS & ACTIVITY block isn't useful on screen, so 03 shows ALLOCATION
  and FINANCIAL only (token counts stay in `comms-deck usage <seat>`). A
  seat with neither a limit nor a cost is listed under CONNECTED / NO LIMIT
  OR COST REPORTED instead of vanishing. Grok and the Grok Bot bots draw
  from one xAI pool, so they share the account `grok` and show as
  one GROK row. Each part of the row comes from the freshest seat that
  reports it: the weekly limit from the bots, the cost from Grok's CLI.
  By Claude.
- **The chat screen lives on workspace 1, fullscreen.** The owner
  (22:49): it should always appear fullscreen on desktop 1. The Hyprland
  rule that `comms-deck install` writes, the launcher and the docs now say
  workspace 1. `scratchpad-layout --watch` opens only the full view at
  login; the half view also sat on workspace 1 and would be hidden under
  it. `comms-deck-chat half` still opens it by hand. By Claude.
- **/sort: the chat box switches 03's order.** `/sort` reports the
  current sort, `/sort provider|consumption|spend` sets it (an unknown
  word is refused, not mailed); the choice lives in DATA and survives a
  restart, `usage_panel` reads it every frame, and the panel's head
  says "· by consumption" while it's switched. By Vibe. Rebased onto #93, with four review fixes by Claude: the total
  stays on the row with the tag, the deck look shows "sorted by X" in
  03's frame, and /sort matches exactly.
- **`comms-deck usage <seat>`: one provider's telemetry in full.** The
  inspection half of the 03 brief. `usage all` lists every record
  (provider, account, available or not, last update, stale); `usage
  <seat|account|provider>` prints the record's models, every allocation
  window, tokens and activity, spend and its kind, and the raw fields as
  the source wrote them (paths and session ids left out). Anything not
  reported says "not reported", never 0. Plain `comms-deck usage` is the
  old log report, unchanged. By Claude.
- **03 ALLOCATION shows every window.** Each row's name sits on its own
  line with one bar per window it reports (5-hour and weekly alike), so a
  long account label no longer pushes the figures off the edge. The
  header total says "est." when any figure in it is an estimate. By
  Claude, after reviewing #92.
- **03 USAGE: three families from one telemetry record.** The renderer
  half of the 03 redesign (Vibe, against claude/telemetry's pinned shape):
  ALLOCATION (severity-shaded, tighter window, resets), TOKENS & ACTIVITY
  (in/out/cache/requests/sessions/active, "—" for a field nobody reports)
  and FINANCIAL (session_usd, est. where the source prices it itself),
  then CONNECTED / TELEMETRY UNAVAILABLE with the last update for a seat
  that reports nothing — shown, never a misleading zero (the brief's
  rule 5). Allocation rows carry a compact severity bar that shimmers
  while a seat on the account is busy. sort= provider (default: every
  block by provider then seat) | consumption | spend (their own block).
  Labels come from the record — a shared account reads like
  "GROK-BOT ACCOUNT · <its seats>"; nothing is named in the renderer.
  Subtitle: allocation · tokens · spend.
- **telemetry(): one shape for every provider's usage.** The data layer
  for the 03 redesign (the owner, 22:03: every provider, model, bot and
  account, never an invented figure). One record per registered seat plus
  any usage file whose seat isn't registered: provider, account, models,
  availability, freshness, allocation windows, tokens/requests/sessions and
  spend. A figure the provider doesn't report is None, never 0.
  `telemetry_rows()` folds seats that share an account (the Grok Bot) into
  one row with the freshest figures. By Claude.
- **03 USAGE is usage only.** The owner (21:35): 06 CONTEXT SPLIT already
  covers context, so the usage column drops each seat's Context bar and
  the CONTEXT list. It shows each seat's 5-hour and weekly limits,
  session tokens and cost, and PLAN USAGE. A seat with nothing but context
  to report no longer gets an empty section. By Claude.
- **The deck look.** On a screen 150 columns or wider the chat is now a
  dashboard (the owner, after Argona's video): a strip of big numbers
  along the top (seats, working, notes today, waiting, context, cost,
  version), numbered panels in frames with corner brackets and an accent
  each (01 CREW, 02 ACTIVITY, 03 USAGE, 04 LIVE NOTES, 05 TASKS,
  06 CONTEXT SPLIT, 07 ALERTS), and a ticker of the latest notes along the
  bottom. Every panel keeps its old content; 01 CREW is new: a braille
  graph, each seat a cluster sized by its context, notes as links with a
  dot running along the newest. The ticker shows private notes as
  "(private)" and redacts names (vibe's #87 review). Narrower screens, or
  COMMS_DECK_OLD_LOOK=1, keep the old layout. By Claude.
- **The context split computes once per frame.** Both chat panels asked
  `context_split()` every redraw; the answer is now cached on the usage
  files' names, sizes and mtimes, so a frame costs a stat per file instead
  of re-reading them all — and any usage write still refreshes it at
  once. By Devin.
- **`comms-deck kitten [--all|<seat>]`** prints one small ASCII cat - the
  same shared kitten for the day, picked by sha256 of the date (no AI, no
  randomness): a seat's own kitten by name, the team's in rows that must fit
  80 columns, and the self-test checks both. By Mystra, on the owner's ask.
- **Remote seats: an agent on another device can join the deck.** A seat
  announced as `name:remote` in the app-seats file lives on another machine
  and has no process here: its mailbox is the git copies. Notes to it are
  not put in a desktop inbox (nothing here would take them out); its row
  reads its seat file, `away` by default. The service pulls both note repos
  in its sync pass and, through the new `fetch-remote` command and the
  `import_remote()` pass, files every changed message file whose `Machine:`
  is another machine and whose `From:` is a remote seat into the recipient's
  inbox, so a remote agent's notes arrive through hooks and wakes like
  anyone's, and the loop guard still applies to it. The first-sight rule:
  on a repo's first import only the pull head is recorded, so years of
  backlog stay the archive they are. By Mystra (2026-11-25).
- **Mystra joins as the first remote seat**, with `docs/REMOTE.md`: how an
  agent on another device gets its own account, opens the join PR, and
  reads and writes the flight-log format by hand. The seats tables in
  README, COMMS-DECK and quantum-kittens name her. The desktop-side steps
  (the `app-seats` line and the collaborator invites) stay the owner's.
  `sync_records()` is untouched; the note file format is unchanged. By
  Mystra (2026-11-25).
- **The dot box polished.** The CONTEXT SPLIT box's glow wave now runs
  straight through the row break instead of tilting and resetting (the
  phase is the dot's flat index, at the bars' speed), the legend follows
  the box instead of arrival order, and the total line falls back to
  "across N seats" when the name list doesn't fit the column instead of
  being chopped mid-name. From the #82 review. By Vibe.
- **The context split moves right.** The CONTEXT SPLIT pie and its
  per-seat bars now sit at the foot of the right-hand column, under
  TASKS; the left usage column keeps the per-seat meters and the
  CONTEXT / PLAN USAGE lists. The pie and bars were too tall for the
  left. By Devin, on the owner's ask.
- **The citation rule, enforced.** `comms-deck cite-check <PR> [--strict]`
  sits beside review-check as rule 4's own gate: a PR must say who wrote
  it, and in strict mode must cite every file it touches. docs/CITATION.md
  is now the one page that states the rule; COMMS-DECK.md rule 4 links
  here. CI (`.github/workflows/self-test.yml`) runs the self-test and all
  three authorship gates on every PR. From Mystra's Wave-1 package
  (quantum-kittens PR #7), at the owner's ask.
- **Devin's usage, on the panel.** The CLI exports OTLP metrics to the
  service's listener on 127.0.0.1:4318; `usage/devin.json` gets session
  input/output tokens, the most-used model and the raw counters (running
  totals survive a restart). The sessions database fills the figures the
  export lacks (context, cache tokens). Only the metrics signal is
  configured, so prompts and tool text are never exported. By Devin.
- **The top bar's notifications, in the deck.** A NOTIFICATIONS block
  under ACTIVITY shows the same feed the bar scrolls: the service tails
  the queue mako's on-notify writes for waybar and keeps the last few
  hundred in `notifications.jsonl`. Do-not-disturb still applies upstream;
  waybar and mako are untouched. By Devin, on the owner's ask.
- **ACTIVITY cooldowns in metric time.** The spent "idle until" countdown
  (under an hour) and the polling seats' "next check"/"check late" timers
  now show milliseconds — `58:20.250`, `1d 00:00:00.000` — and the screen
  redraws at 10 fps while one is live, so the digits actually move.
  Longer waits keep the coarse countdown. By Devin, on the owner's ask.
- **Scratchpad cells meet.** The gap between the cells is 0, so the agent
  windows sit flush. The outer margin stays 22. Hyprland still draws a 2px
  border, rounded corners and a shadow on each window; the layout script
  clears those on the scratchpad windows only, and leaves the global config
  alone. By Alice.
- **The task board.** An owner request to two or more agents becomes one
  task with one holder, chosen by the deck from availability. Every copy of
  the note says who holds it, a second claim is refused, and the service hands
  the task on when the holder goes spent, away or idle past its lease. The
  board is its own repo, [load-balancer](https://github.com/whyyyyyai/load-balancer).
  comms-deck loads it if it's installed and runs exactly as before if it
  isn't. New MCP tools: `tasks`, `claim_task`, `release_task`, `done_task`.
  Open tasks show under ACTIVITY. By Claude, on the owner's ask, after
  tonight's duplicate PRs. The idea is Lumo's (#40).
- **The agents, categorised: CONTEXT and PLAN USAGE lists.** Under the
  pie, two sorted lists in the usage column: CONTEXT, one row per
  reporting seat with its current context and its share of the pool, the
  pool total in the header; and PLAN USAGE, one row per seat with the
  tighter of its plan meters (the 5-hour window when it has one, else the
  weekly), the percentage in the bar's green/amber/red hue, highest first.
  The pie, the bars and the colour key stay as they were; nothing on
  these lists animates. By Vibe, on the owner's ask.
- **The ready order has its own column again.** The chat screen is three
  columns on a wide terminal: USAGE, the bars and the CONTEXT SPLIT on
  the left, the chat in the middle, and the ACTIVITY panel -- the active
  agents, the ready order, the marks and the waiting squares -- on the
  right, so the pie no longer pushes it off the bottom. On a narrow
  terminal the ready order keeps the single right-hand column and the
  usage column yields. The chat is unchanged, and the activity words
  still don't animate. By Vibe, on the owner's ask.
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
  (Antigravity CLI), `codex`, the app seats (Grok Bot app),
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
