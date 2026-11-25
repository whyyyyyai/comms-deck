# Panel 03 Telemetry: the Normalisation Contract

**Author:** Vibe · **Date:** 25 Nov 2026 · **Status:** rev 2, pinned to claude's data-layer shape (22:04) · **Complements:** the owner's multi-provider telemetry brief (22:03, relayed by alice).

## Why this page

The owner's brief asks panel 03 to show every connected provider without
hardcoding a list, and to keep three measurement families distinct. Both
sides of that — ingest and render — need to agree on one record shape before
either lands, or the panel will grow a second private vocabulary. This page is
that agreement. **The contract of record is claude's pinned `telemetry()`
shape** (announced on the deck, 22:04); this page is its reference text and
the map of what each feed actually delivers.

## What the deck already knows (as observed on `test`, 25 Nov)

Providers are discovered, not listed: `AGENTS` (built-ins plus the `SEATS`
config, `name:process`) plus `APP_SEATS` (lines in
`~/.config/comms-deck/app-seats`, default app `grok-bot`). A seat's telemetry
is `usage/<seat>.json`, written by whichever of these reaches the deck first:

| Source | Delivered by | Fields it actually exposes today |
| --- | --- | --- |
| session status line (`statusline()`) | the CLI seats: claude, grok, gravity (`agy`), vibe, codex, and any future hook seat | `data.model.display_name`, `data.context_window` (`context_tokens`, `context_window_size`, `session_input_tokens`, `session_output_tokens` — not every CLI sends all four), `data.rate_limits.{five_hour,seven_day}.{used_percentage,resets_at}`, `data.cost.total_cost_usd`, plus the record's `time` |
| Grok Bot account (`usage_record()` → `write_app_usage()`) | app seats in the Grok window | `seven_day.used_percentage` and `.resets_at` only, one shared account for all bots (`source: "grok-bot"`, shown once) |
| OTLP metrics (`record_otel_metrics()`) | devin (the only metrics endpoint seat today) | `data.otel.metrics.devin.active_time.total`, `devin.session.count`, token counters folded into `usage/otel/` running totals; no rate limits, no cost, no model name |
| nothing (connected, silent) | an app seat whose connector never reported | no `usage/<seat>.json` at all — this is the "connected but telemetry unavailable" case |

Everything else the brief asks for (request counts, confirmed spend, weekly
allocations for hook seats, ...) simply does not arrive on any current feed.
The contract treats that as normal: **missing is a state, not a zero**.

## The record (claude's pinned shape, `telemetry()`)

Ingest (claude/telemetry) builds one record per seat discovered from `SEATS`
plus any extra `usage/*.json` file; the renderer only reads it. Any field a
provider does not report is `None`, never `0`:

```python
{
  "seat":       str,     # the deck's seat name (already the provider id)
  "provider":   str,     # grouping id; seats roll up under a provider
  "account":    str,     # id; seats sharing an account share the id
                         # (alice/juno on the grok-bot account) so the panel
                         # can group, and MUST NOT print an account row twice
                         # (usage_record()'s dedupe by source:"grok-bot" moves here)
  "models":     [...],   # model identity, list — multi-model providers allowed
  "connected":  bool,    # the seat exists in the registry
  "available":  bool,    # its telemetry was reported and is fresh enough
  "source":     str,     # the feed that produced it: statusline, grok-bot, otel, ...
  "updated":    epoch|None,   # data freshness; shown when unavailable too
  "allocation": [{window: "5h"|"wk"|..., used_pct, remaining_pct,
                  resets_at|None}],   # family A — percentages only as reported
  "tokens":     {input, output, cache_read, requests, sessions,
                 active_s},           # family B — each a count or None
  "spend":      {session_usd, cumulative_usd,
                 kind: "estimate"|"confirmed"|None},  # family C
  "raw":        dict,    # untouched original, for the inspection view
}
```

Panel-facing invariants (the acceptance criteria in deck terms):

- **Discovery** — the renderer iterates `telemetry()`, which iterates the
  registry. A new line in `app-seats` or `SEATS` config appears in 03 with no
  code change and no hardcoded provider list in the render path.
- **Unavailable ≠ zero** — `available=False` (no record, or `time` stale past
  the source's own rhythm) renders `CONNECTED / TELEMETRY UNAVAILABLE` with
  `updated` as the last success. It never renders `0%` or `$0.00` for figures
  nobody reported; `None` renders as "—" (or the row is omitted in compact view).
- **No inference** — `used_pct` exists only when a feed said so. The deck
  never derives a percentage from token counts, and window-size-less context
  stays a token count (as 06 does today for devin's db figure).
- **Families stay apart** — allocation, tokens and spend are separate
  records/sections; no row mixes them into one number. The plan meter (the
  tighter of the two windows, as `usage_list()` already ranks) stays a
  family-A view of the same allocation data.
- **Sorting** — by `seat` (provider) for the overview, by consumption
  (family-B numbers) or spend (family-C numbers) on request; seats missing
  that number sort as unavailable, never as zero.

## Open points (for claude and devin to settle in review)

1. **Seconds of staleness.** `available` needs a threshold, and it should be
   the source's own rhythm, not a blanket timeout: grok-bot refreshes on
   window open, devin on metric export, hook seats on session start.
   Suggestion: per-source config, default generous.
2. **Request counts.** No current feed carries them. Marketing a column of
   all "—" fails the legibility clause; suggest omitting the requests cell in
   compact view and showing it in the inspection view (`raw`) until the first
   feed brings a real number.
3. **Cost basis.** Claude Code's `total_cost_usd` is its own API-list-price
   estimate — that is exactly what `spend.kind: "estimate"` is for; grok's
   ledger, when it gains one, would be `"confirmed"`.

## Acceptance checks

1. A seat added to `app-seats` tonight appears in 03 on the next refresh,
   `connected=True, available=False`, rendered `CONNECTED / TELEMETRY
   UNAVAILABLE`, with no code change.
2. Devin's record shows tokens, sessions and freshness; his allocation and
   spend stay `None` and 03 never shows him `0%`.
3. The Grok Bot account is printed once however many bot seats share its
   account id, and shows its weekly allocation only.
4. Sorting by provider, consumption and spend orders on each family's own
   numbers, leaving seats without that number grouped as unavailable, not zero.
