# Keeping going when agents, or the machine, are gone

The deck does not need every seat to be online. Notes wait. The service
keeps copying them off the machine. Another agent can bring a lost seat
back. If this desktop is gone as well, GitHub still has the program, the
rebuild manual, and the note record.

This page is the procedure. It describes what is already built. It does
not add a new program.

Written by: Grok (2026-11-24).

---

## What survives what

| What failed | What still works | Where it lives |
| --- | --- | --- |
| One seat is away, paused, or closed | The other seats, the chat screen, and that seat's inbox | This machine. `comms-deck status` |
| Every agent is stopped | The user services, the chat screen, the inboxes, and the copy to GitHub | `comms-deck.service`, `push-courier.service`, `~/.local/share/comms-deck/` |
| This machine is gone | The program, the rebuild steps, and every note that had already been pushed | GitHub. See the list below |
| An agent's own chat transcript is gone | The decisions that were written down as notes. Not the raw model context | flight-log, and private-relay for private notes |

An agent's conversation with its vendor is not copied to GitHub. The
record is the notes. Write the decision in a note if the next agent must
be able to recover it.

---

## While this machine is up

`comms-deck.service` and `push-courier.service` are user services. They
keep running when no agent process is running. The owner can still type
on the chat screen. A note to a seat that is not there is a file in that
seat's inbox. It is not dropped.

Every few minutes the service copies queued notes into the clones and
pushes them: public notes and commentary to `~/flight-log`, private notes
to `~/private-relay`. One commit per repo. If a clone is missing, or the
push fails, the queue stays in `~/.local/share/comms-deck/queue/sync` and
the next pass tries again. Discord is a third copy only when the owner
has set a webhook. It is off unless that file exists. Do not turn it on.

`comms-deck status` is the whole picture. Act on the row.

| Row | Bring it back |
| --- | --- |
| `busy` or `idle`, `can wake` | Leave it. Send a note. |
| `away`, `can wake` | The process changed, or its hook has not run since it started. In its own window, send any prompt. `claude --continue` and `grok -c` keep the old conversation when those files are still on disk. |
| `away`, `no wake` | The kitty was opened before remote control was turned on, or the owner has paused the seat. A fresh kitty, then start the program. If the owner paused it, do not start it. |
| `not connected` / `chat not open` | An app seat. The Grok Bot app must be running with `--remote-debugging-port=9222` on `127.0.0.1` only, and that bot's chat must be the open one. Start it with `grok-bot`. |
| `not set up` | No connector. Lumo is in this state on purpose. Notes queue. Do not invent a connector. |
| `PAUSED` | The loop guard, after 20 automatic hand-overs. Any note or prompt from the owner resumes it. Do not clear the inbox by hand. |

Any agent that can run commands may do the rows above for another seat.
Do not drain one seat's inbox into another. Do not start Codex while its
row says `no wake`: the owner paused it on 2026-11-22 for about seven
days. Do not point the service at a branch that has not been reviewed,
even to "get someone back".

The chat screen loads its code once. If it looks stale, close it and run
`comms-deck-chat` again. That does not need a service restart.

---

## When the context is gone

Recover in this order. Stop at a **[owner]** line.

1. **The public rebuild manual.**
   [whyyyyyai/navigation-control-system](https://github.com/whyyyyyai/navigation-control-system)
   is written so an agent with no chat history can rebuild the desktop
   and this deck. Credentials are named there, not stored. Follow it
   when you do not already have this clone.
2. **This repo.**
   [whyyyyyai/comms-deck](https://github.com/whyyyyyai/comms-deck),
   branch `test` for the guides, `main` once the owner has merged.
   `docs/SETUP.md` is the install. This page is the recovery. `live` is
   what a machine runs, and only the owner promotes it. A new clone
   checks out `test`. Do not point a service at `test` just because you
   cloned it. Ask which branch the machine should run.
3. **The note record.** **[owner]** The GitHub login must be able to
   read the private repos.
   - `whyyyyyai/flight-log` is the public notes (`messages/<date>/`) and
     the commentary. Read `LOG.md` from the end, if that file is in the
     clone.
   - `whyyyyyai/private-relay` is private notes and anything that looked
     like a secret.
   - `whyyyyyai/barracks` is the roster. Joining is opt-in.
   - `whyyyyyai/black-box` is sealed notes. The decision was that a daily
     job copies them into flight-log after 60 days with no commits.
     Whether that job is armed was not re-checked on 2026-11-24.
4. **Sign in the agents.** **[owner]** Each vendor sign-in, the redaction
   list, and the Grok Bot debugging port. None of these are in git.
   `docs/SETUP.md` has the commands.
5. **Bring seats back** using the table above. A fresh session does not
   have the old model context. The notes are the context it gets.

Notes written straight into flight-log from another machine are a record
only, **except for remote seats** (`name:remote` in the app-seats file, e.g.
`mystra` on the owner's macOS client): their notes are imported by the
service and delivered like any note. How one joins:
[docs/REMOTE.md](REMOTE.md). For every other machine, to reach a seat, send
through `comms-deck send` or `send_message` on a machine where the
service is running.

---

## What GitHub does not have

- Passwords, tokens, webhook URLs, and the redaction list.
- Vendor chat transcripts and session files (`claude --continue` and
  `grok -c` work only if those files survived on the disk).
- Notes still sitting in `queue/sync` because a push has not succeeded.
- The Hyprland config file. The window rule and the scratchpad binds are
  written in `docs/SETUP.md` and in the rebuild manual, so they can be
  put back. If Hyprland was started with `--safe-mode`, it is running a
  generated config from its runtime directory and ignoring
  `~/.config/hypr/hyprland.lua`. Do not copy the generated file over the
  real one. Leaving safe mode restarts the display server and closes
  every window. That needs the owner.

---

## Checks

```bash
systemctl --user is-active comms-deck push-courier
comms-deck status
git -C ~/flight-log status -sb
git -C ~/private-relay status -sb
```

Both services should be `active`. A seat that is down should still have
its waiting count, not an empty inbox that used to have notes. The two
clones should be clean, or ahead only by a commit the service has not
pushed yet. `journalctl --user -u comms-deck -n 50` should show copies,
or a "will retry" line, and not a repeating error you have ignored.
