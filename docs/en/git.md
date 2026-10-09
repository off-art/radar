# Git, diff and approvals

[← README](../../README.en.md) · [Русский](../ru/git.md)

## Git in the list

The second line of each agent shows the repository state in its folder: `⎇ branch ↑ahead ↓behind ●files +lines −lines`
(orange when there are changes). It refreshes every few seconds and right after the agent finishes.
Non-git folders show the path. Data comes from `git status`/`git diff` in a background thread, so the UI never stalls.

## Diff viewer

`Ctrl+b` then `v` (or right click on an agent → "Changes (git diff)…", or the palette) opens a window with changes in the agent's folder
relative to the last commit: files on the left (`M` modified, `A` new, `D` deleted, `R` renamed, `?` untracked) with `+/−` counts,
highlighted lines on the right (green added, red removed).

![Changes](../diff.png)

Keys: `j`/`k` or wheel for lines, `PgUp`/`PgDn`/`Space` for a page, `n`/`p`/`Tab` or click for another file,
`g`/`G` for start and end, `r` to reload, `Esc` to close. New files are shown in full (up to 200 KB), binary files are marked.
If the folder is not a git repository or has no changes, Radar says so.

## Git actions

Right click on an agent or the palette (`Ctrl+b`, `p`). Commands run in the background and the result arrives as a notification:

- **Commit…**: asks for a message and runs `git add -A && git commit` (key `c` inside the diff window).
- **Push…**: `git push` of the current branch after confirmation (key `P` in the diff window). Passwords are never prompted:
  if authentication is needed, Radar shows the git error and nothing hangs.
- **Merge branch into main…** (agents with a worktree): `merge --no-ff` into the main repository; on conflict the merge is rolled back.
- **Remove worktree…**: closes the agent and removes the worktree and its branch (the branch only if already merged).

Dangerous actions always go through a confirmation dialog. For your own shortcuts use the actions `git_commit`, `git_push`,
`git_merge`, `git_remove_worktree` in `[keys]`.

## Event log

![Event log](../log.png)

`Ctrl+b` then `l` (or right click → "Event log…", or the palette) shows the last 200 events: agent started, finished
(with duration), waiting for an answer (with the request text if the agent reported it), exited, and git action results.
Newest first; `↑`/`↓` select, `Enter` or click jumps to the agent. While there are unseen entries, the status line shows "log: N".
The log lives in memory only and is cleared when Radar exits.

## Approving requests from the list

![Approving a request](../approve.png)

When an agent stops at "allow this command?", you can answer without entering it:

- `Ctrl+b` then `y`: Radar takes the selected agent, or the nearest waiting one if it is not waiting (requests also appear in the palette
  as "Approve: …" and in the right-click menu on empty space);
- click the "waiting" line in the list or header; a waiting agent's row shows "y: approve".

The dialog always shows what exactly the agent asks for (`Bash: rm -rf build`) and the question with options as they appear on the agent's screen
(`1. Yes, allow once`, `2. Always allow…`, `4. No`). Choose with `↑`/`↓` (or a digit), confirm with `Enter` or `y`; Radar sends the right number
of arrows and Enter. Any other key (`Esc`) cancels. If the request changes while the dialog is open, Radar sends nothing.
The option the agent highlighted is preselected. There is intentionally no "approve all".

Works for Claude Code and Qwen Code (the request text must come from hooks: for Qwen, the [integration](integrations.md) must be enabled).
For other agents enable it in `config.toml`: `approve = "y"` (or `"enter"`, `"1"`; an empty string disables it). Which key approves depends
on the agent, so enable only what you have verified.
