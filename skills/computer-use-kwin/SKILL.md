---
name: computer-use-kwin
description: "Linux desktop observation and control for KDE Plasma 6 on Wayland via native Pi tools or the computer-use-kwin MCP server."
author: agent-sh
license: MIT
platforms: [linux]
compatibility: "Native Pi tools require Pi 0.84.4+ and Node.js 22.19+; the standalone CLI/MCP server supports Node.js 18+."
---

# computer-use-kwin

Use `computer-use-kwin` to observe or operate a KDE Plasma 6 Wayland desktop:
read application accessibility state, take screenshots, inspect and control
windows, click, scroll, type, press keys, or invoke AT-SPI actions.

## Install

The npm and crates.io packages are not published yet. These registry install
commands are for a future release; use a source checkout in the meantime.

Build the MCP server from the repository root:

```bash
cargo build --locked
target/debug/computer-use-kwin doctor | jq .readiness
```

### Pi native tools

From the same checkout, start Pi with the native extension and point it at the
built server:

```bash
COMPUTER_USE_KWIN_BIN="$PWD/target/debug/computer-use-kwin" \
  pi -e "$PWD/pi/extension/index.ts"
```

This enables Pi's `computer_use_kwin_*` tools. It does not put
`computer-use-kwin` on `PATH`; shell commands below need the CLI install.

### Shell CLI / MCP server

```bash
npm install -g @hydranix/computer-use-kwin
computer-use-kwin doctor | jq .readiness
```

Rust users can install from crates.io:

```bash
cargo install computer-use-kwin
computer-use-kwin doctor | jq .readiness
```

The npm and Cargo commands above will work after the corresponding package is
published. Until then, build the server from source as shown above.

Provide the system-level GTK/AT-SPI prerequisites required by your applications.
There is no application accessibility setup command. XDG portals provide
capture and RemoteDesktop input; allow the requested portal access when
prompted. If using ydotool, run `ydotoold` as a per-user service:

```bash
systemctl --user enable --now ydotoold
```

Direct uinput provides an absolute-pointer fallback and does not require
`ydotoold`. KWin scripting is the sole window-management backend for discovery,
focus, activation, move, and resize operations.

For MCP hosts with `COMPUTER_USE_KWIN_NOTIFY_ON_COMPLETE=1`, call the optional
`complete_interaction` tool once after finishing desktop interaction. A skipped
cue is not a task failure. This notification does not guarantee exclusive
desktop ownership or that other clients have stopped sending input.
It applies only to directly spawned MCP hosts, not the native Pi extension.

## Configure your agent

Configure the binary as a stdio MCP server:

```json
{
  "command": "computer-use-kwin",
  "args": ["mcp"]
}
```

If the binary is not on `PATH`, use its absolute path (typically
`~/.local/bin/computer-use-kwin` or the npm global bin directory). Pi native
tools do not need this MCP configuration; see
[Pi setup](references/pi-setup.md).

### Host-specific guides

- [Hermes setup](references/hermes-setup.md)
- [Pi coding agent setup](references/pi-setup.md)

## Procedure

1. In Pi, call `computer_use_kwin_tools` with the exact tools or capability you
   need. Enabled tools use the `computer_use_kwin_<name>` prefix, appear
   starting on the next model turn, and remain active for the session.
2. Begin each desktop-control turn with `get_app_state`, scoped to the app you
   are working in. Pass `app_name_or_bundle_identifier` or a window target
   (`window_id`, `pid`, `app_id`, `wm_class`, `title`). Without a target, the
   whole desktop AT-SPI tree is returned and may flood context. Use
   `include_screenshot: false` when the tree is sufficient. If
   `accessibility_tree_truncated` is true, narrow the target and raise
   `max_nodes` or `max_depth` rather than lowering them.
3. Use `doctor` when you need the full diagnostic report.
4. If `can_build_accessibility_tree` is false, check system-level GTK/AT-SPI
   prerequisites and restart the target app if needed.
5. Before targeted input, call `list_windows` or `focused_window` and verify
   the intended window by title, app id, pid, or wm class.
6. Prefer semantic targeting from `get_app_state`: use element indices or
   role/name/text/states selectors.
7. Use coordinates only when the UI surface has no useful accessibility tree.
8. For text input, prefer `type_text` with a target selector
   (`window_id`, `pid`, `app_id`, `wm_class`, `title`, `tty`,
   `terminal_pid`, `terminal_command`, or `terminal_cwd`) rather than relying
   on current focus.
9. After mutating actions, re-check state with `get_app_state`,
   `focused_window`, or app-specific readback.

Plain left element/index/selector `click` prefers native AT-SPI `click`,
`press`, or `toggle` over toolkit bounds when available. Use `perform_action`
explicitly for `activate` or `jump`; `click` never substitutes those actions.
Explicit `x`/`y`, right clicks, and double/multiple clicks retain pointer
semantics.

### Screenshot-relative coordinates

For a coordinate `click` or `scroll` with `relative: true`, select a target
window and use its clipped screenshot crop origin. Divide preview `x`/`y` by
the screenshot `scale` first. Widget-local coordinates are not interchangeable
with that origin; missing window targets are rejected. For calibration, use
the repository's `examples/coordinate_probe.py` and require its delivered
event to report `hit: true`.

## Pitfalls

- Running GTK, Qt, and Electron apps may need restarting after accessibility
  prerequisites are changed.
- The first screenshot or `get_app_state` call with screenshots enabled may
  prompt for portal access.
- Desktop input is stateful. Avoid concurrent tool calls against this MCP
  server.
- Pi serializes the native Computer Use tools and keeps one process for the
  session. If that process exits, do not replay an ambiguous mutating call;
  obtain a fresh `get_app_state` first.
- `click`, `drag`, `press_key`, `type_text`, `perform_action`, and `set_value`
  can change real application state.
- When ydotool is selected, `ydotoold` should run as a per-user service with
  its socket under `/run/user/$UID`, not as a system-wide service.
- The optional ydotool backend requires version 1.0.3 or newer; `doctor`
  rejects older or semantically incompatible CLIs even when `ydotoold` runs.
- On Wayland, `COMPUTER_USE_KWIN_PERSIST_REMOTE_DESKTOP=1` opts into reusing portal restore tokens across processes. The first permission dialog still appears; unset is the default and prompts for each new process. Tokens are stored in the user state directory with mode `0600`.

## Verification

Pi-only installs: enable and call `computer_use_kwin_doctor` as in
[Pi setup](references/pi-setup.md). Shell `computer-use-kwin doctor` needs
the CLI on `PATH`.

```bash
computer-use-kwin doctor | jq .readiness
```

Check that the readiness report lists the needed capabilities without
blockers, then call `list_windows` to inspect the active desktop session.
