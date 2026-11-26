# Load Balancer: Routes Reviews Too

**Author:** Lumo (lumo seat) · **Date:** 24 Nov 2026 · **Status:** rev 2, after claude's review on #40
**Complements:** the owner's announced load balancer — task routing by availability.

## Problem

This morning's bottleneck was not writing code. Features landed fast all day (quantum-kittens #35, the availability ordering, the six-word lines, alice's boot-loop fix #37). What stalled was **review throughput**: everything queued behind a two-person reviewer pool (claude and grok), while gravity, codex and lumo sat outside it. Grok was flagged "urgent, please review #37" while the machine's other seats idled. A task nobody verifies is not done, so routing reviews is not a nice-to-have — it is the load balancer's core job.

## Requirements

1. **Reviews are tasks.** The router treats "review PR N" as a routable task, weighted by urgency (age × merge-blocking flag), not by arrival order.

2. **Reviewer pool by capability, not availability alone.** The pool is **a config list, not code** (default: claude, grok, gravity, lumo — the seats that can actually open a diff), so resuming Codex later is a one-line change. **The change's author is never in its pool** (review-check already fails a `Reviewed by` naming the `Written by` seat). Codex stays out while the owner has paused it; the Grok Bot chat seats stay out — they are chat seats, not reviewers. Availability orders *within* the pool; it does not define the pool.

3. **Pool never empties; rank, don't skip.** Rank the pool: ready > busy > stuck > idle-until. A stuck seat (idle with notes waiting) is handed work last, not never — skipping stuck seats can drain the pool and recreate the stall the router exists to fix. Reuse the deck's `availability()` and `most_available(seats=pool)` so panel and router cannot disagree. Escalate to the owner only when every seat in the pool is spent.

4. **Claims are announced, acknowledged, revocable.** The router claims a review by writing `reviews/<pr>.json` *before* announcing it, so two agents never both start. A claim lapses on silence — about 10 minutes for terminal seats, longer for polling seats — and the router **re-claims and announces the move** without asking permission: an unacknowledged claim is not ownership. A reviewer accepts by saying so (a note naming the PR), not by the mere existence of the file.

5. **The reviewer writes their own `Reviewed by:` line.** This is deck law already; the router must never stamp approval on a reviewer's behalf, and must never auto-approve after a timeout. Silence is not sign-off.

6. **Routing is visible.** Every assignment appears on the chat screen like a note does: who has it, since when, and what unblocks it. Reassignments are announced too. Invisible routing is indistinguishable from dropped work.

7. **Nothing merges by machine.** The owner merges. Chronophage *files, offers and reminds* — it does not push to main, ever, and a reminder is never treated as consent.

## Non-goals

- Waking a spent agent early (Chronophage already refuses to wake spent seats).
- Routing `@deck` desktop actions (that mechanism stays as built).
- Replacing `team_status` etiquette notes — those stay for humans; the router is for claims and hand-offs.

## Shape

Not a new daemon: **one step in Chronophage's one-minute pass**. Chronophage already reads `comms-deck status --json`, holds the wake path, and has report-once/hourly escalation; `gh pr list` is enough to watch the queue (`push-courier` is for sandboxed pushes, not reads). On each pass: rank the pool, offer the oldest unclaimed review, write the claim file, announce, and lapse stale claims.

## Acceptance checks

1. *The morning's failure:* #37 lands urgent, the router offers it to the highest-ranked available reviewer, marks it claimed, and on silence re-offers to the next seat within minutes instead of waiting for a human.
2. *Late commits:* a reviewer signs off, then a new head lands (as with #35 today): the claim reopens for the new head and the old sign-off does not carry over.
