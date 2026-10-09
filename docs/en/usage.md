# Using the UI

[← README](../../README.en.md) · [Русский](../ru/usage.md)

```sh
radar                        # open the UI, add agents with n
radar ~/work/api claude      # open and start Claude Code in a folder
radar claude claude codex    # three agents in the current folder
radar ~/work/api gigacode    # GigaCode CLI in a project folder
```

Control is by **mouse** and **keyboard** (modeled on Herdr).

## Mouse

- `Left click`: select an agent; `+ new` in the list header starts a new one; `notifications: on` at the bottom toggles them.
- `Right click` (or Ctrl+click): context menu. On an agent: rename, restart, mute notifications for it only,
  new agent in this folder, close. On empty space: new agent, grid, notifications, help.
- Wheel scrolls history. In `less`/`vim`/`htop` the wheel acts as arrow keys; agents that enable the mouse themselves
  (e.g. OpenCode) receive the events as they are.
- **Drag** the border between the list and the agent window; the width is remembered (min 20 columns, the agent keeps at least 40).

## Keys

Press **`Ctrl+b`** once to enter *navigation mode* (a yellow "NAVIGATION" badge at the bottom). Nothing needs to be held down;
the mode stays on until you press `Esc`/`Enter`.

| Key | Action |
|---|---|
| `j` / `k` (`↓` / `↑`, `Tab`, `(` `)`) | next / previous agent |
| `1…9` | jump to an agent by number |
| `n` / `N` | new agent / new agent of the same kind in this folder |
| `x` | close the agent |
| `r` / `R` | rename / restart an exited agent |
| `w` | jump to the agent that is **waiting for an answer** |
| `g` | grid ⇄ single agent |
| `m` / `M` | mute the selected agent / all notifications |
| `u` `d` (`[` `]`, `PgUp` `PgDn`) | scroll history |
| `y` | **approve** the request of a waiting agent ([details](git.md#approving-requests-from-the-list)) |
| `l` | **event log** ([details](git.md#event-log)) |
| `v` | **agent's changes** (`git diff`) ([details](git.md#diff-viewer)) |
| `G` / `a` / `o` | create a group / move the agent into a group / collapse-expand its group |
| `K` / `J` | move the selected agent up / down (order is remembered; also by dragging and via right click) |
| `,` | **settings** (`Tab` switches to the Integrations tab) |
| `S` | choose a sound theme (with preview) |
| `p` or `Space` | **command palette**: search actions and agents |
| `?` · `q` | help · quit |
| `Q` | stop all background agents and quit |
| `Ctrl+b Ctrl+b` | send a literal `Ctrl+b` to the agent |

Without the prefix, `Shift+↑` / `Shift+↓` (switch agents) and `Shift+PgUp` / `Shift+PgDn` (scroll) work.
All shortcuts can be changed in `[keys]` ([Settings](config.md)): change the prefix (`ctrl+space`, `ctrl+a`…)
and add direct shortcuts (e.g. `"alt+n" = "new"`). While there are no agents, `n`, `p`, `?`, `q` work without the prefix.

Forms and input fields support normal editing: `←/→`, `Home/End`, `Option+←/→`, `Ctrl+a/e/u/k/w`.

## "New agent" form

Shows only installed agents (Radar checks in the background at startup); the `+ N more (not installed)` button reveals the rest.
Pick an agent by click, `←/→`, a digit, or `a` (show/hide unavailable ones); clicking the Folder, Name or worktree checkbox row
jumps to it. Move between fields with `↑`/`↓` or a click. Right click on empty space also offers "New: <agent>" for every installed agent.

- **Inline hint.** While you type a path, the first matching option is shown in gray; `→` accepts it.
- **Folder completion.** `Tab` works like in a shell: completes the path (`~/Desk` → `~/Desktop/`); with several matches it fills the common
  prefix and lists options under the field. Case-insensitive, `~/` is optional: `desk` + `Tab` → `~/Desktop/` (current folder first, then home).
  Hidden folders are suggested if you start the name with a dot.
- **Worktree.** The checkbox creates a separate git worktree for the agent so several agents do not edit the same files.

## Groups

You create groups yourself; agents are never grouped automatically. Agents without a group appear below all groups under "No group".

- **Create**: the `+ new group` button at the bottom of the list, `Ctrl+b` `G`, or right click on empty space. In the dialog enter a name and tick agents
  (`Tab` switches between name and list, `Space` ticks, `Enter` confirms). An **empty** group is allowed.
- **Add an agent**: `Ctrl+b` `a` (or right click → "Add to group…"), or drag the agent onto a group header.
- **Rename, change members, move, delete**: right click on the group header. Deleting a group leaves its agents in the list ungrouped.
- **Collapse / expand**: click the header or `Ctrl+b` `o`. The header shows a summary: `▾ Backend 3  ⠋ 1 ● 1 ✓ 1` (working / waiting / ready).
- `K`/`J` move an agent only within its group. Groups and collapsed state are remembered between runs.

All small dialogs (confirmations, group dialog, rename, palette, request approval) respond to clicks:
on `Enter` / `Esc` buttons, list rows and checkboxes; clicking outside the dialog cancels it.

## Copying text

Drag with the left button over text in the agent window: the selection is highlighted and, when you release the button,
copied to the clipboard (via `pbcopy` on macOS, otherwise OSC 52). "Copied: N chars" appears at the bottom.
Selection also works where the agent captures the mouse itself (Qwen Code, OpenCode) and is cleared by any key, click or scroll.
A plain click still goes to the agent. Selection is linear, like in a terminal; agent frame characters (`│`) are included in the copy.
While a selection is visible, right click offers "Copy".

Disable it: `Ctrl+b ,` → "Mouse selection (copy)" or `mouse_select = false` in `config.toml`; then the mouse goes entirely to the agent
(needed for vim, mc). With the mouse fully off (`mouse = false`), the terminal's native selection works. You can bypass Radar with
Option held (iTerm2) or Fn (Terminal.app), but that also selects the agent list.
