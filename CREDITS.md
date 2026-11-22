# Who wrote what

Several AI agents (Grok, Codex and Claude) build this repo together for
the repo's owner. This file records who wrote each part, so credit stays with the
right agent as the code changes hands.

**Joint work** means the agents built it together, and nobody recorded at
the time who wrote which lines. Those files are credited to all three
agents, and no agent claims them alone.

## The programs

| File | Written by | Notes |
| --- | --- | --- |
| [`relay-protocol`](relay-protocol) | **Joint: Grok, Codex, Claude** | The relay itself was built together before 2026-11-20. On 2026-11-20 Claude added the explanatory comments (no behaviour changed), the rename from `other-brain`, GitHub sync, the commentary, and public-by-default with `--private`. On 2026-11-21 Codex tested the GitHub sync for real. |
| [`agent-chat/agent-chat`](agent-chat/agent-chat) | **Joint: Grok, Codex, Claude** | Built together before 2026-11-20. Claude added the comments on 2026-11-20, with no code changes. |
| [`agent-chat/install.sh`](agent-chat/install.sh), [`agent-chat/test.sh`](agent-chat/test.sh) | **Joint: Grok, Codex, Claude** | Claude added the header comments on 2026-11-21. |
| [`relay-watch`](relay-watch) | Claude (2026-11-20) | |
| [`push-courier`](push-courier), [`systemd/push-courier.service`](systemd/push-courier.service) | Claude (2026-11-21) | Reviewed by Grok (2026-11-22); its findings were fixed before it landed here. |
| [`scheduler`](scheduler) | Claude (2026-11-21), from the owner's design | Reuses the joint `relay-protocol` code. Codex has proposed a different back-off on her branch `codex/chat-log-sync`. |
| [`systemd/relay-protocol.service`](systemd/relay-protocol.service) | **Joint: Grok, Codex, Claude** | Claude added the comments on 2026-11-21. |
| [`systemd/relay-scheduler.service`](systemd/relay-scheduler.service) | Claude (2026-11-21) | |

## The docs

| File | Written by |
| --- | --- |
| [`README.md`](README.md) | Joint: Grok, Codex, Claude (Claude wrote the current text) |
| [`docs/RELAY-PROTOCOL.md`](docs/RELAY-PROTOCOL.md) | Joint: Grok, Codex, Claude, before 2026-11-20 (partly out of date) |
| [`agent-chat/README.md`](agent-chat/README.md), [`agent-chat/AGENTS.md`](agent-chat/AGENTS.md) | Joint: Grok, Codex, Claude |
| [`docs/BRANCHING.md`](docs/BRANCHING.md) | Claude (2026-11-20), from the owner's plan for test → main → live |
| `CREDITS.md` (this file) | Claude (2026-11-21) |
| The puzzle footer in `README.md`, [`puzzle/check.py`](puzzle/check.py) | Claude (2026-11-21), from the owner's idea |
| [`LAUNCHPAD.md`](LAUNCHPAD.md) | Claude (2026-11-22), from the joint work above |
| [`recall`](recall) | Claude (2026-11-22), at the owner's request |
| [`PRIMER.md`](PRIMER.md) | Claude (2026-11-22), from the team's record and the joint work of Grok, Codex and Claude |

## Work in progress

Open work, and who it belongs to, is listed in
[LAUNCHPAD.md](LAUNCHPAD.md#5-where-things-stand-as-of-2026-11-22). When a piece
lands here, add its row to the tables above.

## About the commit history

This repo starts from a single fresh commit that gathers the work done in
earlier, now private, repos. The credits above carry that work's
authorship forward. From here on, each commit names its agents in
`Co-Authored-By:` lines.

## Keeping this up to date

When you add or substantially change a file:

1. Put a `Written by:` line at the top of the file (in its "read this first"
   comment) or at the bottom of a doc.
2. When you build on someone else's file, **keep their credit** and add
   yours with what you changed and the date, as `relay-protocol` does.
3. Update the table here.
4. Add one `Co-Authored-By:` line per agent to the commit.
5. Agents who only reviewed go under `Reviewed by:`.

---

Written by: Claude
