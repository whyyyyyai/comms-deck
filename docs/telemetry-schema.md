# Panel 03 Telemetry: the Normalisation Contract

**Author:** Vibe · **Date:** 25 Nov 2026 · **Status:** rev 1, for devin's ingest and claude's panel to aim at
**Complements:** the owner's multi-provider telemetry brief (22:03, relayed by alice).

## Why this page

The owner's brief asks panel 03 to show every connected provider without
hardcoding a list, and to keep three measurement families distinct. Both
sides of that — ingest and render — need to agree on one record shape before
either lands, or the panel will grow a second private vocabulary. This page is
that agreement. It is a spec, not code: nothing here changes the deck's
behaviour until devin's ingest PR and claude's panel PR implement it.

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

Everything else the brief asks for (request counts, confirmed-vs-estimated
spend, weekly allocations for hook seats, ...) simply does not arrive on any
current feed. The contract treats that as normal: **missing is a state, not a
zero**.

## The record

One `telemetry(seat)` record per registered seat. Ingest owns building it;
the panel only renders it. Fields the ingest cannot fill are **absent**, or
`null` where presence itself is meaningful (a null stays distinguishable from
an absent key only inside ingest — by the time the panel sees the record both
mean "not reported").

```python
{
  "seat":        str,     # the deck's seat name (already the provider id)
  "provider":    str,     # same as seat today; exists so a seat could front several
  "account":     str,     # "self" by default; "grok-bot shared" for the bot account;
                          # devin's is its workspace — more than one account per
                          # provider becomes a list of records, one per account
  "model":       str|absent,
  "freshness":   {"updated": epoch, "age_s": float},
  "allocation":  [        # family A. list, because a seat can have both windows
    {"window": "five_hour"|"seven_day"|"weekly",
     "kind":   "subscription"|"rolling",     # brief's distinction, kept explicit
     "used_pct":       float,                # only as reported
     "reset_at":       epoch|absent},
  ] |absent,
  "activity":    {        # family B. every member independently absent
    "input_tokens":  int, "output_tokens": int,
    "requests":      int, "sessions": int,
    "active_s":      float,
    "context":       {"tokens": int, "window_size": int},
  } |absent,
  "financial":   {        # family C
    "total_cost_usd": float, "session_cost_usd": float|absent,
    "basis":  "reported"|"cli-estimate",    # claude code's figure is an estimate
  } |absent,
  "telemetry":   "ok"|"unavailable",
}
```

Panel-facing invariants (the acceptance criteria in deck terms):

- **Discovery** — the panel iterates the same `SEATS` tuple everything else
  does. A new line in `app-seats` or `SEATS` config appears in 03 with no code
  change and no hardcoded list in the render path.
- **Unavailable ≠ zero** — a seat with no `usage/<seat>.json` (or one whose
  `time` is stale past the source's own refresh rhythm) renders
  `CONNECTED / TELEMETRY UNAVAILABLE` with the age of any last success. It
  never renders `0%` or `$0.00` for figures nobody reported.
- **No inference** — `used_pct` exists only when a feed said so. The deck
  never derives a percentage from token counts, and `window_size`-less
  context stays "tokens only" (as 06 does today for devin's db figure).
- **Families stay apart** — a percentage is allocation, a token count is
  activity, a dollar figure is financial; no row mixes them into one number,
  and the plan meter (the tighter of the two windows, as `usage_list()`
  already ranks) stays inside family A.

## Open points (for claude and devin to settle in review)

1. **Seconds of staleness.** "Stale" should mean the source's own rhythm, not
   a blanket timeout: grok-bot refreshes on window open, devin on metric
   export, hook seats on session start. Suggest: unacceptable staleness is
   per-source config, default generous.
2. **Request counts.** No current feed carries them. Either the panel shows
   the column with every cell "—" until a feed provides it, or it omits the
   column until the first arrival. The brief's legibility clause suggests
   omitting, and adding it back when a provider brings one.
3. **Basis tagging.** Claude Code's `total_cost_usd` is its own API-list-price
   estimate — the `financial.basis` field is how the panel can show `est.`
   honestly without the ingest guessing.

## Acceptance checks

1. A seat added to `app-seats` tonight appears in 03 on the next refresh, rendered
   `CONNECTED / TELEMETRY UNAVAILABLE`, with no code change.
2. Devin's record shows activity, freshness and no allocation or financial
   section, and 03 never shows him `0%`.
3. The Grok Bot account appears once, weekly allocation only, shared label,
   regardless of how many bots sit in `app-seats`.
4. Sorting by provider, consumption and expenditure orders on each family's own
   numbers, leaving seats without that number grouped at "unavailable", not zero.
