# Automation and background sessions

[← README](../../README.en.md) · [Русский](../ru/automation.md)

## Background sessions

Each agent runs in its own background process (`radar host`); the Radar window just attaches to it.
**Quit Radar (`q`) and the agents keep working**: tasks run, conversations stay alive, shell variables and background commands survive.
Run `radar` again and the window reattaches with the same screen and state.

- An agent ends when you close it (`Ctrl+b`, `x`), when it exits on its own, or with **`radar stop`**
  (stops all background agents). `Ctrl+b`, `Q` stops everything and quits.
- Hooks and integrations keep working: events go through the agent process and reach the window after reattaching.
- One agent is shown in one window: a second Radar shows only its own agents and warns that the others are open elsewhere.
- `radar ctl` drives the window; to see what runs in the background without a window, open `radar`.
- On attach the visible screen is restored; scrollback accumulated before the window was closed is not kept.
- If the Mac was rebooted or agent processes were killed, Radar restarts agents from the saved list (`sessions.toml`),
  and **Claude Code resumes its previous conversation** (`claude --resume`); other agents start fresh. Running `radar <agent>`
  with arguments skips restoring. Disable: Settings → "Restore agents" or `restore = false`.
- `radar stop` does not clear the saved list: agents are started again on the next launch.

## Scripting (`radar ctl`)

While Radar is open you can drive it from another terminal or a script:

```sh
radar ctl list [--json]                       # agents, statuses, branches
radar ctl new claude ~/proj/api --name api    # start an agent (--worktree: in a separate worktree)
radar ctl send api "run the tests and fix"    # send text and Enter (--no-enter: without Enter, "-": from stdin)
radar ctl status api [--json]                 # starting / working / waiting / idle / exited
radar ctl read api --lines 30                 # last lines of the agent's screen
radar ctl wait api --timeout 600              # wait until the agent finishes
radar ctl close api
```

Refer to an agent by number (from `list`), name or name prefix. `wait` exits with `0` (ready), `3` (waiting for an answer),
`4` (exited) or `124` (timed out); to avoid catching a stale state right after `send`, it waits until the agent has been calm for
`--settle` seconds (default 1.5). Example: start two agents and wait for both:

```sh
radar ctl new claude . --name review && radar ctl send review "review the last commit"
radar ctl new claude . --name tests  && radar ctl send tests  "write tests for src/auth"
radar ctl wait review; radar ctl wait tests; radar ctl read review --lines 40
```

Commands go through a unix socket with owner-only permissions (`0600`). If several Radar windows are open, the latest one is used;
be explicit with `--sock PATH`. Inside an agent started by Radar, `RADAR_SOCK` already points to its own window.
