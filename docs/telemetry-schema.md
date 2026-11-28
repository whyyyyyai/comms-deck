# Panel 03 Telemetry: the Normalisation Contract

**Author:** Vibe · **Date:** 25 Nov 2026 · **Status:** rev 5, pinned to claude's data-layer shape (22:04) as landed in #91, updated for #94/#96/#98 and #99 · **Complements:** the owner's multi-provider telemetry brief (22:03, relayed by alice).

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
| Grok pool (`usage_record()` → `write_app_usage()`, plus Grok's own CLI) | the app seats in the Grok window, and the `grok` CLI seat | the bots: `seven_day.used_percentage` and `.resets_at`; the CLI: session cost. One shared account `grok` (`SHARED_ACCOUNTS`), one `GROK` row |
| OTLP metrics (`record_otel_metrics()`) | devin (the only metrics endpoint seat today) | `data.otel.metrics.devin.active_time.total`, `devin.session.count`, token counters folded into `usage/otel/` running totals; no rate limits, no cost, no model name |
| nothing (connected, silent) | an app seat whose connector never reported | no `usage/<seat>.json` at all — the "connected but telemetry unavailable" case |

**Optional feed-declared overrides (as of #99).** A usage feed may declare
fields in its `data` that the deck would otherwise infer from the seat and
source:

- `data.provider` — the provider name, e.g. `"Acme Cloud"`. It wins over the
  `PROVIDER_LABELS` fallbacks, so a renamed or new provider needs no deck
  change. Must be a non-empty string; anything else is ignored.
- `data.account` or `data.account_id` — the account id the feed draws from.
  It wins over the `SHARED_ACCOUNTS` mapping. Numbers are stringified;
  booleans and empties are not ids.
- `data.models` — a list of model ids/names, for providers with several.
  It wins over the single `data.model.*` fallback.

Overrides change only the labels the record carries — never figures, never
the family separation. They also exist because **a transport is not a
provider**: a remote app seat's program reads `remote`; without a declared
`data.provider` its provider renders honestly as `unknown`, not as the
transport name.

Everything else the brief asks for (confirmed spend, weekly allocations for
hook seats, ...) simply does not arrive on any current feed.
The contract treats that as normal: **missing is a state, not a zero**.

## The record (claude's pinned shape, `telemetry()`)

Ingest (claude/telemetry) builds one record per seat discovered from `SEATS`
plus any extra `usage/*.json` file; the renderer only reads it. Any field a
provider does not report is `None`, never `0`:

```python
{
  "seat":       str,     # the deck's seat name (already the provider id)
  "provider":   str,     # grouping id; seats roll up under a provider
                         # (data.provider wins, then PROVIDER_LABELS by
                         # source/program, then source; "remote" is a
                         # transport and reads "unknown")
  "account":    str|None, # id; seats sharing one pool share the id
                         # (data.account, else SHARED_ACCOUNTS: grok CLI and
                         # the bots are account "grok"), so the panel can
                         # group pool seats into one row
  "models":     [...],   # model identity, list (data.models, else the
                         # status line's model) — multi-model allowed
  "connected":  bool,    # the seat exists in the registry
  "available":  bool,    # it reports at least one usage figure at all
  "stale":      bool,    # updated is older than TELEMETRY_STALE (6h for now)
  "source":     str,     # the feed that produced it: statusline, grok-bot, otel, ...
  "updated":    epoch|None,   # data freshness; shown when unavailable too
  "allocation": [{window: "5h"|"wk"|..., used_pct, remaining_pct,
                  resets_at|None}],   # family A — percentages only as reported
  "tokens":     {input, output, cache_read, requests, sessions,
                 active_s},           # family B — each a count or None
  "spend":     {session_usd, cumulative_usd,
                 kind: "estimate"|"reported"|None},  # family C
  "raw":        dict,    # untouched original, for the inspection view
}
```

`telemetry_rows()` is the fold the panel reads. Records whose seats share an
**account id within one provider** become **one row** — as of #98 the fold
is part-wise, each family taking the freshest report that has it (Grok's CLI
supplies the spend, the bots the weekly allocation), and the row's
`updated`/`stale`/`available` cover the whole pool (`stale` only when every
part is). As of the #99 review, the grouping key is
`(provider, account)`: a declared account id shared by two different
providers must stay two rows, never one merged pool. A seat with no account
is its own row, `seats == [seat]`.

Panel-facing invariants (the acceptance criteria in deck terms):

- **Discovery** — the renderer iterates `telemetry()`, which iterates the
  registry. A new line in `app-seats` or `SEATS` config appears in 03 with no
  code change and no hardcoded provider list in the render path.
- **Unavailable ≠ zero** — on the 03 panel, a seat with neither an
  allocation nor a spend renders under `CONNECTED / NO LIMIT OR COST
  REPORTED` with `updated` as the last success ("never reported" when there
  is none). It never renders `0%` or `$0.00` for figures nobody reported;
  `None` renders as "—" (or the row is omitted in compact view). In the
  inspection view (`usage <seat>`/`usage all`, #94) the same state is
  spelled `telemetry CONNECTED / TELEMETRY UNAVAILABLE`; the panel phrase
  changed with #98 only because family B left the screen.
- **No inference** — `used_pct` exists only when a feed said so. The deck
  never derives a percentage from token counts, and window-size-less context
  stays a token count (as 06 does today for devin's db figure).
- **Families stay apart** — allocation, tokens and spend are separate
  records/sections; no row mixes them into one number. On the panel as of
  #98, family B is **inspection-only**: 03 shows ALLOCATION and FINANCIAL
  from `telemetry_rows()`, while `comms-deck usage <seat>` still prints the
  token/activity record in full. The plan meter (the tighter of the two
  windows, as `usage_list()` already ranks) stays a family-A view of the
  same allocation data.
- **One pool, one row** — seats on a shared account render once under the
  pool's label (`GROK`), pooling parts by freshness within one provider
  only.
- **Sorting** — by `seat` (provider) for the overview, by consumption
  (family-A percentages) or spend (family-C figures) on request; seats
  missing that number sort as unavailable, never as zero.

## Open points (for claude and devin to settle in review)

Settled by #91, kept here for the record: staleness is one
`TELEMETRY_STALE` (6h) threshold, the `stale` flag; request counts come
only from Devin's db (`devin.requests`) and are `None` everywhere else;
cost `kind` is `"estimate"` (Claude Code prices the session itself) vs
`"reported"` (a ledger the source owns). Settled by #98: the fold is
part-wise; family B left the panel for the inspection view and the quiet
list became NO LIMIT OR COST REPORTED.

Still open:

1. **Seconds of staleness, per source.** One 6h threshold works today, but
   the refresh rhythms differ (grok-bot on window open, devin on metric
   export, hook seats on session start). Per-source config, if a real seat
   ever reads stale while being fresh.
2. **In-panel expandable detail.** Inspection (`usage <seat>`) is
   CLI-only; 03 has no expand/collapse per row yet. Settling it changes
   only the renderer — the record shape here is already enough for it.

## Acceptance checks

1. A seat added to `app-seats` tonight appears in 03 on the next refresh,
   `connected=True, available=False`, listed under `CONNECTED / NO LIMIT OR
   COST REPORTED`, with no code change.
2. Devin's record shows tokens and freshness; `requests` comes from
   `devin.requests`, `sessions` only when OTLP exports
   `devin.session.count` (either may be `None`, never faked); his
   allocation and spend stay `None` and 03 never shows him `0%`.
3. The GROK pool is printed once however many seats share account `grok`
   (the CLI and the bots): the bots' weekly allocation and the CLI's
   session cost share the row, each taken from the freshest reporter.
4. Sorting by provider, consumption and spend orders on each family's own
   numbers, leaving seats without that number grouped as unavailable, not
   zero.
5. Two providers declaring the same `data.account` render as two rows —
   pooling is scoped to (provider, account), never across providers.
