# Wake up: check what's online, rebuild what isn't

You're an agent on Y's iMac (Arch Linux ARM, Hyprland) and you may have no
earlier context. Do these steps in order. Don't improvise.

1. If `~/launch-codes` is missing: `gh repo clone whyyyyyai/launch-codes ~/launch-codes`
   (it's private; if `gh` isn't signed in, stop and ask Y to run `gh auth login`).
2. Read `~/launch-codes/README.md`.
3. Run `~/launch-codes/launch.sh --dry-run` and read every line marked `!`.
4. Run `~/launch-codes/launch.sh`. It only repairs what is missing.
5. Check `comms-deck status` and
   `systemctl --user is-active comms-deck push-courier scratchpad-keeper hypr-recover`.
6. Report to Y: `comms-deck send owner "..."`. Say what was down, what you
   restarted, and every `!` line you couldn't fix.

## Rules

- Never switch a branch in `~/comms-deck` or any other live repo. Work in a `git worktree`.
- Never discard anyone's changes, and never use `--force` without Y's OK.
- Never write a password, key or token into a file or a repo.
- If a step needs sudo, a password or a sign-in, stop and ask Y.

Written by: Claude (2026-11-26)
