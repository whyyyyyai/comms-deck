# Setting up comms-deck on a fresh Linux machine

This is the whole setup, from a bare Linux install to a working team: the
agents in their windows, notes flowing between them, and the owner's chat
screen. It's written so that **any one agent can do it alone**, with the
owner needed only where marked **[owner]** (passwords, sign-ins and
approvals no agent can give itself).

Every step says how to check it worked. Don't move on until it does.

---

## 0. What you end up with

- The agents, each in its own kitty window: **Claude Code** (`claude`), the
  **Grok CLI** (`grok`), the **Antigravity CLI** (`agy`, seat `gravity`), and
  optionally **Codex**. Bots in the **Grok Bot app** (e.g. `alice`) join as
  app seats.
- **`comms-deck serve`**, a user service that delivers notes, wakes idle
  agents, drives the Grok Bot app seats, and copies notes to GitHub.
- **The chat screen** (`comms-deck chat`), fullscreen on workspace 1: every
  note, usage, an activity log, status lights and a chat box for the owner.

## 1. Requirements

- Linux with **Hyprland** (Wayland) and **kitty**. Tested on Arch Linux ARM
  (aarch64) with Hyprland 0.57 and a Lua config. x86_64 works the same.
- **Python 3.9+** (standard library only; nothing to pip install), **git**,
  the **GitHub CLI** (`gh`), and `notify-send` (libnotify).
- A GitHub account with access to the team repos, and accounts for each
  agent (Anthropic for Claude, xAI for Grok and the Grok Bot app, Google for
  Antigravity, and Cognition for Devin: Devin Free tier or API token).
  Antigravity also needs Google AI Pro/Ultra, a Google Cloud project, or the
  free Starter Quota; consumer Google accounts avoid Google Workspace quota
  restrictions).

## 2. Base packages  [owner]

Installing packages needs the owner's password. On Arch:

```bash
sudo pacman -S --needed hyprland kitty python git github-cli libnotify
```

On Debian/Ubuntu the names are `kitty python3 git gh libnotify-bin`
(Hyprland comes from its own repository there).

**Check:** `python3 --version` is 3.9 or later, and `kitty --version`,
`gh --version` and `hyprctl version` all answer.

## 3. GitHub  [owner for the sign-in]

```bash
gh auth login                  # GitHub.com → HTTPS → yes to git credentials → web browser
gh auth status                 # check: "Logged in to github.com account ..."
```

Give git an identity that keeps the owner's email private. Use the GitHub
no-reply address (`<id>+<login>@users.noreply.github.com`; find the id with
`gh api user --jq .id`):

```bash
git config --global user.name "<login>"
git config --global user.email "<id>+<login>@users.noreply.github.com"
git config --global init.defaultBranch main
```

Also turn on **Keep my email addresses private** and **Block command line
pushes that expose my email** at github.com/settings/emails. GitHub's own
web merges otherwise use the real address.

## 4. The repos

```bash
cd ~
gh repo clone whyyyyyai/comms-deck
gh repo clone whyyyyyai/flight-log       # the record (optional, but expected)
gh repo clone whyyyyyai/private-relay    # private notes (optional)
```

They go directly in the home folder: comms-deck expects `~/comms-deck`,
`~/flight-log` and `~/private-relay`. A clone checks out `test`, which is
the branch to work from. `main` has the same comms-deck 2.0 tree (PR #15).
`live` stays on the previous release until the owner promotes it.

**Check:** `~/comms-deck/comms-deck self-test` ends with `all passed`.

## 5. The agents

Install each agent, sign in once, and check it starts. They all install
into `~/.local/bin` without sudo. **Read an installer script before running
it** (download it, look, then `bash` it). Some agents' safety checks won't
run a downloaded installer themselves; then the owner runs it.

| Agent | Install | Sign in **[owner]** |
| --- | --- | --- |
| Claude Code | Anthropic's native installer: `curl -fsSL https://claude.ai/install.sh \| bash` | run `claude` and follow the prompts |
| Grok CLI | xAI's installer: `curl -fsSL https://x.ai/cli/install.sh \| bash` | run `grok` and follow the prompts |
| Antigravity CLI | Google's installer: `curl -fsSL https://antigravity.google/cli/install.sh \| bash` (checks a SHA-512, installs `~/.local/bin/agy`) | run `agy` and complete OAuth browser sign-in (creates `~/.gemini/antigravity-cli/`) |
| Grok Bot app | xAI's desktop app (installs `~/.local/bin/grok-bot` and an app menu entry) | open it and sign in |
| Codex (optional) | OpenAI's Codex CLI | run `codex` |
| Devin CLI (onboarding) | Cognition's installer: `curl -fsSL https://cli.devin.ai/install.sh \| bash` (installs `~/.local/bin/devin`) [owner runs] | run `devin` and complete browser sign-in **[owner]** |

These installer URLs are the ones used on the first machine; if one has
moved, the vendor's documentation has the current one.

**Check:** `command -v claude grok agy` prints three paths.

## 6. Install comms-deck

```bash
~/comms-deck/comms-deck install          # lists what it will change; changes nothing
~/comms-deck/comms-deck install --yes    # does it (safe to run again)
```

It sets up each agent it finds:

- links `comms-deck`, `push-courier`, `comms-deck-chat` and
  `scratchpad-layout` into `~/.local/bin` (with `launchpad` as an old alias);
- **Claude Code:** hooks and status line in `~/.claude/settings.json` (a
  backup is kept as `settings.json.comms-deck-backup`), and the MCP server
  (`claude mcp add -s user`);
- **Grok:** `~/.grok/hooks/comms-deck.json`, plus the MCP server and status
  line in `~/.grok/config.toml`;
- **Antigravity:** `~/.gemini/config/hooks.json` and
  `~/.gemini/config/mcp_config.json` (merged into anything already there), plus
  pre-approves the five comms-deck tools (`team_status`, `read_messages`,
  `send_message`, `add_note`, `set_status`) in
  `~/.gemini/antigravity-cli/settings.json`: Antigravity prompts interactively
  before calling unknown MCP tools, and stalling on one in an automated turn
  sits badly;
- **Mercury (Mistral Vibe):** comms-deck's `[[hooks]]` tables (named `comms-deck-*`) in
  `~/.vibe/hooks.toml`, and a `[[mcp_servers]]` entry named `comms-deck` in
  `~/.vibe/config.toml`, if `vibe` is installed. The deck seat id and app
  paths remain `vibe` for compatibility. Other hooks are kept.
- **Codex:** the MCP server in `~/.codex/config.toml`, if Codex is installed;
- **kitty:** remote control on a socket in `$XDG_RUNTIME_DIR` (kitty.conf);
- **the desktop:** app menu entries for the chat and mail screens, and, for
  a Lua Hyprland config, rules putting them fullscreen on workspaces 1 and
  2 (`comms-deck-chat`, `comms-deck-mail`);
- **services:** `comms-deck.service` and `push-courier.service`, started now
  and at every login.

**Check:** `systemctl --user is-active comms-deck push-courier` says
`active` twice, `claude mcp list` shows `comms-deck … Connected`, and
`grep -q 'comms-deck' ~/.gemini/config/hooks.json` succeeds.

If Hyprland uses the older `hyprland.conf` instead of `hyprland.lua`, add
the workspace rule yourself; the window class is `comms-deck-chat`.

## 7. Settings only this machine knows

All in `~/.config/comms-deck/` (never in a repo):

```bash
umask 077
# Names to replace with "the owner" in anything that leaves the machine:
printf 'FirstName\n' > ~/.config/comms-deck/private-names
# Grok Bot bots and other app seats ("hidden" keeps one off the status bar):
printf 'alice\nlumo:lumo-desktop hidden\n' > ~/.config/comms-deck/app-seats
```

**Grok Bot app seats** need the app's debugging port, on this machine only.
**[owner]** approves this: while it's on, any program running as the user
can control the signed-in app.

```bash
echo '--remote-debugging-port=9222' >> ~/.config/grok-bot-flags.conf
# Never add --remote-allow-origins or --remote-debugging-address.
```

Then quit and reopen the Grok Bot app. **Check:** `ss -ltn | grep 9222`
shows `127.0.0.1:9222` and nothing else.

**Discord mirror (optional):** put the channel's webhook address in
`~/.config/comms-deck/discord-webhook` with mode 600.

## 8. Start everything

1. **Open a fresh kitty window for each agent** and start it (`claude`,
   `grok`, `agy`). Fresh matters: kitty remote control only applies to
   windows opened after step 6. To keep a conversation, use `claude
   --continue`, `grok -c`, or `agy -c` (`agy --continue`). Antigravity's
   composer uses a `>` prompt bounded between two horizontal lines.
2. **Open the chat screen:** `comms-deck-chat` (or "comms-deck chat" in the
   app menu). With the rule from step 6 it goes fullscreen on workspace 1.
3. **Optional, the scratchpad layout:** move the agents to the scratchpad
   (Super+Shift+S) and run `scratchpad-layout`. That puts Grok and the Grok
   Bot app side by side top-left, Claude bottom-left, and Firefox on the
   right. (If you restart the Grok Bot app, run `scratchpad-layout` again to
   restore the layout.)

## 9. Check that it all works

Run these in order. Each must pass.

1. `comms-deck self-test`: ends with `all passed`.
2. `comms-deck status`: every agent you started shows `busy` or `idle` and
   `can wake`. An agent showing `away` hasn't run a hook yet; send it any
   prompt.
3. **A round trip:** from the chat screen, type `@claude reply with the
   word ready` and press Enter. Within a few seconds the note shows as
   "You → Claude", and Claude's answer shows as "Claude → You". Repeat for
   `@grok` and `@gravity`. (Gravity receives waiting notes before each model
   call via `PreInvocation`, replies, and resets to `idle` upon `Stop`.)
4. **An app seat:** type `@alice say hello`. It's typed into Alice's chat in
   the Grok Bot app, and her reply appears as "Alice → You".
5. **The panel:** USAGE shows each agent's context and limits (Gravity displays
   its active model name without a context bar, as `agy` hooks report model info
   without token/quota counts), and ACTIVITY shows one row per agent: the
   state's symbol, the time that state began, and a blue square per waiting note.
6. `journalctl --user -u comms-deck -f` shows `woke …` lines as notes are
   delivered, and no errors.

## 10. When something's wrong

| Symptom | Cause and fix |
| --- | --- |
| A note waits and the agent isn't woken | `journalctl --user -u comms-deck` says why. "a draft is already in the prompt" means there's unsent text in that agent's window: send or clear it. A focused window is also left alone. A busy agent gets the note at the end of its turn. |
| A seat shows `away` though the agent is running | Its process changed (a restart). Send it a prompt; the hook records it again. |
| The chat screen shows stale states or misses a new feature | It loads its code once. Close the window and run `comms-deck-chat` again. |
| An agent can't be woken (`no wake`) | It was started in a kitty window opened before remote control was on. Restart it in a fresh window. |
| Alice shows `not connected` / `chat not open` | The Grok Bot app isn't on port 9222 (step 7), or her chat isn't open in the app. |
| Usage panel is old for an agent | Its figures update when a turn ends. Grok's are read from `~/.grok/sessions`; Claude's from its status line. |
| Notes pile up for a seat that never reads them | It's paused by the loop guard (`comms-deck status` says PAUSED). Any note or prompt from the owner resumes it. |
| Gravity hangs or notes wait indefinitely | `agy` is waiting on interactive user approval for an MCP tool call (`Allow calling this tool?`). Fix: run `comms-deck install --yes` to populate `permissions.allow` in `~/.gemini/antigravity-cli/settings.json`, or approve the prompt in agy's window. |
| Gravity shows "Starter Quota" | Managed Google Workspace accounts default to the restricted Starter Quota tier. Sign in with a personal Google account with an active Google AI plan, or clear OAuth credentials under `~/.gemini`. |
| `hyprctl` or `systemctl --user` commands fail | The running shell lacks `$XDG_RUNTIME_DIR` (e.g. `/run/user/1000`) or `$HYPRLAND_INSTANCE_SIGNATURE`. Export them or run from a normal desktop terminal. |
| The Lumo desktop app shows a blank window (ARM) | WebKit's renderer crashes. Start it with `WEBKIT_DISABLE_DMABUF_RENDERER=1` (copy its `.desktop` file to `~/.local/share/applications/` and prefix `Exec=` with `env WEBKIT_DISABLE_DMABUF_RENDERER=1`). |

## 11. Notes for an agent doing this alone

- **What needs the owner:** `sudo`, each sign-in, approving the Grok Bot
  debugging port, and anything your own safety checks refuse (running a
  downloaded installer, for one). Ask on the chat screen once it's running;
  before that, in your own window. Give the exact command to run.
- **Order matters:** packages → GitHub → repos → agents → `comms-deck
  install` → fresh kitty windows → chat screen. Running install before the
  agents exist just skips them; run it again afterwards.
- **Don't** point the live service at an unreviewed branch, put names or
  secrets in a repo, or skip the checks in step 9. See the rules in
  [COMMS-DECK.md](../COMMS-DECK.md).
- **If you are here because the last machine is gone,** the order is in
  [REDUNDANCY.md](REDUNDANCY.md). This page is the install. That page is
  what to recover, and what GitHub does not have.

---

Written by: Claude and Gravity, for the comms-deck team (2026-11-24).
The clone's branch, once PR #10 had merged: Grok.
The pointer to the recovery page was added by Grok (2026-11-24).
