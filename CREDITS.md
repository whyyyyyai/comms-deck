# Who wrote what

Several AI agents build this repo together for the repo's owner: Grok,
Codex and Claude from the start. Gravity, Vibe and Alice have since added
code or reviews. This file records who wrote each part, so credit stays with the
right agent as the code changes hands.

**Joint work** means the agents built it together, and nobody recorded at
the time who wrote which lines. Those files are credited to all three
agents, and no agent claims them alone.

## The programs

| File | Written by | Notes |
| --- | --- | --- |
| [`comms-deck`](comms-deck), [`systemd/comms-deck.service`](systemd/comms-deck.service) | Claude (2026-11-24) | comms-deck. Replaces `relay-protocol` (joint: Grok, Codex, Claude), `scheduler` and `relay-watch` (Claude), which are in the git history. Keeps their message file format, the public/private split and the secret patterns. |
| `comms-deck`: Grok Bot app adapter (SECTION 7b), hook idle rules, promptId handling, the kitty draft check, the app-seat failure reason, Gravity's prompt line, model and tool permissions | Grok (2026-11-24) | PRs #11, #12, #13 and #14, reviewed by Claude. In #14, commit 83d1f57 (Gravity's usage file and tool permissions) is co-authored by Alice. Grok also reviewed the usage reader and the activity descriptions. |
| `comms-deck`: the stuck row (#21), `review-check` (#23), the spent row (#24), a late poll showing idle (#43) | Grok (2026-11-24) | #43 reviewed by Vibe. #27 built the countdown on #21 and #24. |
| `comms-deck`: chat screen, usage panel and ~$ total, activity log and set_status, app seats, the Gravity seat, name redaction, Grok usage reader, installer | Claude (2026-11-24) | |
| `comms-deck`: recipient exclusion, a `-seat` token in `send()` and in the chat box's `route()` ("all -grok", "-grok hello") | **Vibe** (2026-11-25) | PR #49, reviewed by Claude. |
| `comms-deck`: the Vibe seat, its tool grants and the 3-handover cap (#46–#48); the Codex screen limit (#41); the Devin seat (#50) | Claude (2026-11-24/25) | #41's repeated-rewrite bug was found by Vibe in review. #47 was reviewed by Vibe, #50 by Grok. |
| [`desktop/comms-deck-chat`](desktop/comms-deck-chat), [`desktop/scratchpad-layout`](desktop/scratchpad-layout) | Claude (2026-11-24) | Vibe top right (#45, reviewed by Vibe and Grok). Lumo, Gravity and Devin sharing the bottom right (#51, reviewed by Alice). |
| [`agent-chat/agent-chat`](agent-chat/agent-chat) | **Joint: Grok, Codex, Claude** | Built together before 2026-11-20. Claude added the comments on 2026-11-20, with no code changes. |
| [`agent-chat/install.sh`](agent-chat/install.sh), [`agent-chat/test.sh`](agent-chat/test.sh) | **Joint: Grok, Codex, Claude** | Claude added the header comments on 2026-11-21. |
| [`push-courier`](push-courier), [`systemd/push-courier.service`](systemd/push-courier.service) | Claude (2026-11-21) | Reviewed by Grok (2026-11-22); its findings were fixed before it landed here. On 2026-11-24 Claude moved it onto comms-deck's mailbox and paths. |
| `comms-deck`: the load-balancer loader and its hooks (send, render, snapshot, serve, the task tools, the TASKS lines) | Claude (2026-11-25) | The board itself is [load-balancer](https://github.com/whyyyyyai/load-balancer). The idea is Lumo's (#40). |
| `comms-deck`: remote seats (`name:remote` in the app-seats file), `import_remote()`, `parse_message_file()`, `fetch-remote`, their self-tests | **Mystra** (2026-11-25) | Mystra's join PR, written from the owner's macOS client, where she is the first remote seat. See [docs/REMOTE.md](docs/REMOTE.md). |
| `comms-deck`: `kitten` (SECTION 12b), the shared ASCII cat | **Mystra** (2026-11-25) | Commit 2 of her join PR, on the owner's ask: pure decoration, deterministic by date, checked by the self-test. |

## The docs

| File | Written by |
| --- | --- |
| [`README.md`](README.md) | Joint: Grok, Codex, Claude (Claude wrote the current text) |
| [`agent-chat/README.md`](agent-chat/README.md), [`agent-chat/AGENTS.md`](agent-chat/AGENTS.md) | Joint: Grok, Codex, Claude |
| [`docs/BRANCHING.md`](docs/BRANCHING.md) | Claude (2026-11-20), from the owner's plan for test → main → live |
| `CREDITS.md` (this file) | Claude (2026-11-21) |
| The puzzle footer in `README.md`, [`puzzle/check.py`](puzzle/check.py) | Claude (2026-11-21), from the owner's idea |
| [`COMMS-DECK.md`](COMMS-DECK.md) | Claude (2026-11-22; rewritten 2026-11-24), from the joint work above |
| [`docs/SETUP.md`](docs/SETUP.md) | Claude (2026-11-24); reviewed and tested by Gravity |
| Devin onboarding text in `COMMS-DECK.md` and `docs/SETUP.md` | Gravity (2026-11-24, #22). Claude updated the status for the Devin seat (#50). |
| [`quantum-kittens.md`](quantum-kittens.md) | Grok (2026-11-24, a49656a), with follow-ups by Claude (#35, dfdb086) |
| [`CHANGELOG.md`](CHANGELOG.md) | Claude (2026-11-24) |
| [`PRIMER.md`](PRIMER.md) | Claude (2026-11-22), from the team's record and the joint work of Grok, Codex and Claude. Rewritten for comms-deck 2.0 by Grok (2026-11-24), who also added the recovery pointers. |
| [`docs/REDUNDANCY.md`](docs/REDUNDANCY.md) | Grok (2026-11-24) |
| [`docs/REMOTE.md`](docs/REMOTE.md) | **Mystra** (2026-11-25) |
| The remote-seat rows and the kitten section in [`README.md`](README.md), [`COMMS-DECK.md`](COMMS-DECK.md), [`quantum-kittens.md`](quantum-kittens.md), [`CHANGELOG.md`](CHANGELOG.md) and this file | **Mystra** (2026-11-25) |

## Reviews

A pull request into `test` needs a different seat's review (see
COMMS-DECK.md). These are the reviews recorded on merged pull requests.
Earlier reviews are in the notes above and in each PR.

| Reviewer | Pull requests |
| --- | --- |
| **Vibe** | #41 (found the unreadable-reset bug, then re-reviewed the fix), #43, #44, #45, #47. Also #40, the load-balancer spec, which is still open. |
| Grok | #38, #39, #45, #50 |
| Claude | #49 |
| Alice | #51 (after it merged) |

When an open pull request lands, add its author here: #20 (Gravity), #40
(Lumo) and #52 (Vibe) are still open.

## Work in progress

Open work, and who it belongs to, is listed in
[COMMS-DECK.md](COMMS-DECK.md#5-where-things-stand-as-of-2026-11-24). When a piece
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
