# Left Panel design (vibe) — for claude's t87 review

**Author:** vibe · **Date:** 28 Nov 2026 · **Status:** rev 1, inspiration doc for t87 (claude builds her own from this + codex's)
**Brief from Y:** include all metrics, visualize the data, graphs/charts/physics, occasional mouse-click interactivity. Right panel stays comms/activity/notify.

## Frame

- Left panel, full height under the top bar, width `[panel].width` (default 300 px), own background `[panel].bg` slightly darker than `[look].background`.
- Three tabs on F4/F5/F6 (claude's proposal, kept): **F4 SYSTEM**, **F5 TASKS**, **F6 CREW**. Default tab SYSTEM; current tab in the health line. Tab switch is a soft fade, never a layout jump.
- Viewer law: the panel never sends a note by itself; only explicit clicks act (below).

## F4 SYSTEM — machine numbers, all of them

Top row of big numbers: fps, worst frame (ms), stall count, RAM used/total, swap used/total, load 1/5/15, power (on battery + %).

Charts (ring buffer of the last 120 samples at the 2 s status tick, drawn as sparklines):

- **fps sparkline** with a threshold line at `[look].fps`; stalls marked as red ticks on the time axis.
- **frame-time histogram**: last 120 frames bucketed into 12 bins, drawn as bars — the shape of the stalls is visible at a glance.
- **RAM and swap sparklines**, stacked area, swap in amber; swap bar turns red above 80% used. Given tonight's 2G/4G with 71 MiB RAM free, swap gets its own row, not a footnote.
- **load sparkline** (1/5/15 as three thin lines).
- **physics flourish:** a spring gauge for memory pressure — a needle on a dial whose restoring spring is the actual RAM+swap usage; it oscillates and settles as pressure changes. Purely decorative, driven by real numbers.

## F6 CREW — every seat, including hidden

One row per seat in deck roster order: colour dot + pose word (ready/stuck/busy/away/spent/idle-until/hidden), name, waiting count + oldest age (e.g. `3 · 12m`), ctx % as a thin bar, five-hour/week as two mini arcs, held task tag `tNN`.

- Hidden seats (lumo, t81) are shown dimmed with a `hidden` chip — visible but parked.
- **Click a row** → `comms-deck wake <seat>` and focus that seat's window (the occasional interactivity; the deck's own rules still decide waking, e.g. never wake a spent agent).
- Hover a row → exact numbers (bytes, seconds, percentages).

## F5 TASKS — the board

List of `tNN`: holder, open/held, waiting/oldest, title (truncated). The focused seat's held tasks highlighted. **Click a task** → compose a note to its holder through the existing chat route (owner→seat), never auto-send.

## Data sources (read-only)

`~/.local/share/comms-deck/status.json` (seats, machine numbers), `tasks/*.json` (board), `usage/<seat>.json` (ctx/limits), `log.jsonl` (frame/stall history comes from the warp core's own HUD counters, not the deck).

## Config keys ([panel] in warp-core.toml)

`enable = true`, `width = 300`, `tab_keys = ["F4","F5","F6"]`, `spark_samples = 120`, `hist_bins = 12`, `bg = [0.008,0.01,0.025]`, `accent = "seat"` (or a fixed colour), `click_wake = true`.

---

Written by: vibe (2026-11-28), t87 inspiration round.
