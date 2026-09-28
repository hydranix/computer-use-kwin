<div align="center">
  <h1>computer-use-linux for KDE Plasma 6</h1>
  <p><strong>Control a KDE Plasma 6 Wayland desktop from an MCP host.</strong></p>
  <p>
    <a href="https://github.com/agent-sh/computer-use-linux/actions/workflows/ci.yml"><img src="https://github.com/agent-sh/computer-use-linux/actions/workflows/ci.yml/badge.svg" alt="CI"></a>
    <a href="https://crates.io/crates/computer-use-linux"><img src="https://img.shields.io/crates/v/computer-use-linux.svg" alt="crates.io"></a>
    <a href="https://www.npmjs.com/package/@agent-sh/computer-use-linux"><img src="https://img.shields.io/npm/v/@agent-sh/computer-use-linux.svg" alt="npm"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-MIT-yellow.svg" alt="License: MIT"></a>
  </p>
</div>

> ⚡ Running this agent 24/7? [**tiyuvta inference**](https://inference.tiyuvta.ai) — hosted LLM inference built for always-on agents, OpenAI/Anthropic-compatible APIs.

`computer-use-linux` is focused on KDE Plasma 6 on Wayland. It reads shared
AT-SPI accessibility trees, captures through XDG desktop portals, and controls
windows through KWin scripting.

```bash
npm install -g @agent-sh/computer-use-linux
computer-use-linux doctor | jq .readiness
```

The Rust crate is published as [`computer-use-linux`](https://crates.io/crates/computer-use-linux) and the npm wrapper as [`@agent-sh/computer-use-linux`](https://www.npmjs.com/package/@agent-sh/computer-use-linux). Prebuilt binaries ship with the [latest release](https://github.com/agent-sh/computer-use-linux/releases/latest).

## What this is

`computer-use-linux` is a Rust MCP server and CLI for KDE Plasma 6 on Wayland.
Any MCP host — Codex Desktop's Linux build, Claude Desktop, [Hermes
Agent](https://github.com/NousResearch/hermes-agent), or your own client — can
spawn it to read accessibility trees, list and focus windows, take screenshots,
click, drag, scroll, type, and invoke semantic accessibility actions.

The build is scoped to KDE Plasma 6 Wayland and uses its shared desktop
services:

- **Portal capture and input.** XDG desktop portals provide screenshots and
  RemoteDesktop input when available and authorized.
- **Wayland input fallbacks.** Pointer input can use `ydotool` or a direct
  uinput absolute-pointer device. Text entry uses KDE clipboard typing, with
  portal and ydotool input paths available as appropriate.
- **KWin window management.** KWin scripting lists, focuses, activates, moves,
  and resizes windows; it is the sole window-management backend.
- **Shared AT-SPI state.** Semantic selectors and app state use the session's
  shared accessibility services; system-level GTK/AT-SPI prerequisites are
  configured outside this application.
- **One JSON readiness report.** `computer-use-linux doctor` reports portal,
  AT-SPI, windowing, and input readiness with explicit blockers.

The crate was extracted from [`codex-desktop-linux`](https://github.com/avifenesh/codex-desktop-linux) (the Linux distribution of Codex Desktop), which still bundles this binary as a built-in plugin. This standalone repo is the upstream.

## Features

MCP tools exposed by the server:

**Diagnostics**

- `doctor` — single-shot JSON readiness report for portals, accessibility, KWin windowing, and input

**Discovery**

- `list_apps` — running desktop apps visible to the AT-SPI registry
- `list_windows` — windows with title, app id, wm_class, focus state, client type, and bounds
- `focused_window` — the window currently holding keyboard focus
- `get_app_state` — combined screenshot + accessibility tree for a chosen app, with element indices that the input tools accept. Scope it with `app_name_or_bundle_identifier` or a window target; an unscoped call returns the whole desktop tree, reports `tree_scoped: false`, and warns
- `screenshot` — capture the screen as a bounded PNG or JPEG image; can target a window, which is raised to the front and cropped to just that window

Screenshot payloads are size-bounded by default before they are returned to the MCP host: max 1920 px width/height and 2 MiB image bytes, with hard caps even when callers request more. Agents that need more detail can pass `max_width`, `max_height`, `max_bytes`, `scale`, `format: "jpeg"`, or `quality`, preferably with a window target or crop. PNG remains the default; JPEG lets callers trade lossless pixels for a smaller payload before the byte cap forces further resizing. Returned screenshot metadata includes `coordinate_width`, `coordinate_height`, `scale`, `format`, and `quality` so callers can convert from a downscaled preview to desktop coordinate pixels.

**Input**

- `click` — by element index, semantic selector, or desktop coordinate pixels
- `drag` — desktop coordinate drag (start / end)
- `scroll` — page-based scroll on an element or at a pixel location
- `press_key` — keys / chords; can focus a window or terminal first
- `type_text` — literal text input, optionally targeted at a window or terminal

For a plain left `click` by element index or selector, a recognized native
AT-SPI `click`, `press`, or `toggle` action takes precedence
over bounds. Entry `activate` and slider `jump` must be requested explicitly
with `perform_action`; they are never a substitute for a pointer click, even
when bounds are missing.
This avoids pointer conversion for GTK3 HiDPI
extents and GTK4 zero-origin bounds when the element exposes such an action.
The preference does not substitute an arbitrary action name for a coordinate
click. Explicit `x`/`y`, right clicks, and double/multiple clicks retain pointer
semantics. Re-check application state after either kind of activation.

For coordinate `click` or `scroll` with `relative: true`, select a target window
and measure from its clipped screenshot crop origin. Divide preview `x` and
`y` by the returned screenshot `scale` before passing them. These are not raw
GDK surface or widget-local coordinates; decorations and clipping can change
the origin. A missing window target is rejected.

Targeted `press_key`/`type_text` results append focused-element feedback from AT-SPI (role, name, editable) and warn when no editable element holds focus. Click/screenshot/input results warn when the target window or coordinate is partially or fully off-screen. `get_app_state` returns a compact readiness block by default; pass `verbose: true` for the full diagnostics report. It also reports `tree_scoped` (false when no app target narrowed the AT-SPI tree, with a warning in `message`) and `accessibility_tree_truncated` (true when the node, depth, or read budget stopped traversal with unread elements left).

**Semantic actions**

- `perform_action` — invoke any AT-SPI action exposed by an element (`Press`, `Activate`, `Toggle`, …); defaults to the primary action
- `set_value` — write to a settable accessibility element (text fields, sliders, spinners)

**Navigation**

- `activate_window` — focus a window by `window_id`, `pid`, `app_id`, `wm_class`, `title`, or terminal selectors
- `move_window` / `resize_window` — reposition or resize a window in desktop coordinates through KWin scripting

**Conditional host execution**

- `complete_interaction` - optional desktop completion notification, registered only with `COMPUTER_USE_LINUX_NOTIFY_ON_COMPLETE=1`. Repeated calls can create repeated notifications; it does not provide desktop exclusivity.

- `run_shell` — same-user `/bin/sh -c` execution without login-profile loading, registered only when the server operator starts the MCP process with `COMPUTER_USE_LINUX_ENABLE_SHELL=1`. It is deliberately absent by default and is not a sandbox.

### MCP safety contract

`computer-use-linux` is not a read-only data source. It can observe the local desktop and, when a mutating tool is called, can change real application state. The `tools/list` response includes MCP `ToolAnnotations` so hosts can surface this distinction before invocation:

| Class | Tools | Contract |
| --- | --- | --- |
| Read-only observation | `doctor`, `list_apps`, `list_windows`, `focused_window`, `get_app_state` | `readOnlyHint=true`; may reveal app, window, accessibility, and screenshot contents. `get_app_state` may trigger the desktop screenshot portal prompt. |
| UI state mutators | `activate_window`, `move_window`, `resize_window`, `scroll`, `screenshot` | `readOnlyHint=false`, `destructiveHint=false`; changes focus, geometry, or scroll position in the live desktop, or raises a window to capture it. |
| Desktop action mutators | `click`, `drag`, `press_key`, `type_text`, `perform_action`, `set_value` | `readOnlyHint=false`, `destructiveHint=true`, `openWorldHint=true`; can trigger arbitrary actions in whatever local application is targeted. |
| Conditional host-code execution | `run_shell` | Absent unless `COMPUTER_USE_LINUX_ENABLE_SHELL=1`; when enabled, `readOnlyHint=false`, `destructiveHint=true`, `idempotentHint=false`, `openWorldHint=true`. Runs with the MCP server user's host permissions. |

Annotations are safety hints, not an authorization system. MCP hosts should still ask the user before calls that could submit, delete, send, purchase, overwrite, or otherwise commit state.

`run_shell` is an explicit trust-boundary opt-in, not a restricted command runner. Enabling it grants an approved MCP call the same file and network authority as the user running the server. The tool clears the ambient environment and inherits only a small desktop/runtime allowlist (`PATH`, home/user/locale fields, display/session-bus fields); additional variables must be supplied in the visible call payload. Commands use a fixed non-login `/bin/sh`, an existing canonical working directory, a 30-second default / 120-second hard timeout, process-group cleanup, and stderr audit records keyed by the command SHA-256 rather than command text. Collected streams up to 8 MiB are returned with a 512 KiB per-stream response cap and truncation flag; exceeding 8 MiB on either stream fails the call without partial output. These controls bound accidental leakage and runaway work; they do not make arbitrary shell code safe.

The binary also exposes the same capabilities from the CLI for scripting and debugging:

```
computer-use-linux mcp                                  # stdio MCP server
computer-use-linux doctor                               # JSON readiness report
computer-use-linux apps
computer-use-linux state [APP_NAME]
computer-use-linux screenshot                           # JSON screenshot summary
computer-use-linux windows
```

## Support matrix

| Desktop/session | Window backend | Scope |
| --- | --- | --- |
| KDE Plasma 6 on Wayland | KWin scripting | Sole supported target and window-management backend; screenshots and RemoteDesktop input use XDG portals, with ydotool and direct uinput pointer fallback. Shared AT-SPI provides application state. |

If you run on a desktop not covered above, or a covered backend does not come up cleanly, please open an issue with the output of `computer-use-linux doctor` so we can extend the matrix honestly.

## Install

### Option A — `./install.sh` from a clone

Installs build dependencies on supported Debian/Ubuntu, Fedora/RHEL-like, or
Arch-like systems; installs Rust if needed; builds the binary into
`~/.local/bin`; optionally configures `ydotoold` as a systemd user service; and
runs `doctor`. It does not configure toolkit accessibility for you.

```bash
git clone https://github.com/agent-sh/computer-use-linux
cd computer-use-linux
./install.sh
computer-use-linux doctor | jq .readiness
```

`ydotool` is an optional input fallback; direct uinput supplies absolute
pointer input. The installer skips automatic daemon setup on non-systemd hosts
and prints manual `ydotoold` guidance. For an unrecognized distro, pass
`--package-manager apt|dnf|pacman`; `--force-unknown-distro` auto-selects only
when exactly one of those managers is available.

### Option B — `cargo install` (Rust binary)

Installs the Rust binary from crates.io. Ensure your system provides the KDE
desktop portals and the GTK/AT-SPI accessibility prerequisites required by
your applications. No project accessibility setup command is needed or
provided.

```bash
cargo install computer-use-linux
computer-use-linux doctor
```

For unreleased changes from `main`, install directly from Git:

```bash
cargo install --git https://github.com/agent-sh/computer-use-linux
```

Then, as needed:

```bash
sudo apt install at-spi2-core                 # plus your distro's GTK accessibility packages
sudo apt install ydotool                      # optional input fallback
systemctl --user enable --now ydotoold         # only when using ydotool
```

### Option C — npm wrapper (binary download)

Good for users who already have Node.js and want a no-Rust install. The npm
package downloads and verifies the matching Linux binary during install.

```bash
npm install -g @agent-sh/computer-use-linux
computer-use-linux doctor
```

You still need the system-level GTK/AT-SPI prerequisites for your applications
and a supported input path. Start `ydotoold` only when using ydotool.

### Option D — prebuilt binaries

Linux x86_64 / aarch64 builds are published with each tag. Each binary ships a `.sha256` next to it.

- Latest release: <https://github.com/agent-sh/computer-use-linux/releases/latest>

```bash
target=x86_64-unknown-linux-gnu
base=https://github.com/agent-sh/computer-use-linux/releases/latest/download
asset="computer-use-linux-$target"
curl -L -O "$base/$asset"
curl -L -O "$base/$asset.sha256"
sha256sum -c "$asset.sha256"
install -m 0755 "$asset" "$HOME/.local/bin/computer-use-linux"
```

You still need KDE desktop portals and the system-level GTK/AT-SPI
prerequisites required by your applications. Run `ydotoold` only when using
that input fallback.

## Wire it into your MCP host

The binary speaks the `rmcp` 2024-11-05 stdio protocol. Pass `mcp` as the only argument; everything else is configured through MCP tool calls.

### Codex Desktop (Linux build)

The Linux build of Codex Desktop already bundles this binary as a plugin. You don't need to wire it up manually — the plugin definition lives in [`codex-desktop-linux`](https://github.com/avifenesh/codex-desktop-linux) under its `plugins/` directory and is enabled by default. To upgrade the plugin in place, replace the binary it ships with the one from this repo's release assets.

### Claude Code (CLI)

Use the `claude mcp add` command to register the binary as a stdio MCP server. Pick a scope:

- `--scope user` — available across all projects for your user.
- `--scope project` — written to `.mcp.json` at the project root for team sharing.
- `--scope local` (default) — only the current project, stored in `~/.claude.json`.

```bash
# User-wide install (recommended for desktop control)
claude mcp add --scope user computer-use-linux -- computer-use-linux mcp

# Verify the server is registered and reachable
claude mcp list
```

If `computer-use-linux` is not on `PATH`, pass the absolute path (e.g. `~/.local/bin/computer-use-linux`). Inside a Claude Code session, run `/mcp` to confirm the tools are loaded.

### Claude Desktop

Edit `~/.config/Claude/claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "computer-use-linux": {
      "command": "computer-use-linux",
      "args": ["mcp"]
    }
  }
}
```

Restart Claude Desktop. The tools should appear in the tools list.

### Pi Coding Agent

```bash
pi install npm:@agent-sh/computer-use-linux
```

Restart Pi or run `/reload`. The package exposes one small loader initially;
the real tools keep their upstream schemas and are enabled only when Computer
Use is needed:

Native tools require Pi 0.84.4 or newer (Node.js 22.19 or newer). The
standalone npm CLI wrapper continues to support Node.js 18 or newer.

```
computer_use_linux_tools({ tools: ["doctor", "list_windows"] })
computer_use_linux_doctor({})
computer_use_linux_list_windows({})
```

You can also search by capability:

```
computer_use_linux_tools({ query: "observe a window and click a control" })
```

No separate MCP adapter or manual MCP configuration is required. Pi starts one
computer-use-linux process lazily on the first real tool call, reuses it for the
session so accessibility snapshots remain valid, serializes desktop actions,
and closes it on reload, session switch, or exit. See the
[Pi setup guide](skills/computer-use-linux/references/pi-setup.md) for migration
from older adapter-based installs.

### Hermes Agent

Install the companion Hermes skill so Hermes has the desktop-specific runbook:

```bash
hermes skills tap add agent-sh/computer-use-linux
hermes skills install agent-sh/computer-use-linux/computer-use-linux
```

The skill is optional but recommended for Hermes users. It teaches Hermes how to install, configure, verify, and call the Linux desktop MCP safely. It follows the same `skills/<name>/SKILL.md` tap layout used by Hermes community skills.

Then add the stdio MCP server:

```bash
hermes mcp add computer-use-linux --command computer-use-linux --args mcp
hermes mcp test computer-use-linux
hermes mcp configure computer-use-linux
```

`configure` opens Hermes' tool-selection UI for the server. The generated config should look like this:

```yaml
mcp_servers:
  computer-use-linux:
    command: computer-use-linux
    args: ["mcp"]
    timeout: 120
    connect_timeout: 30

# Optional: expose the tools to subagents as well.
inherit_mcp_toolsets: true
```

If you installed the binary somewhere that is not on `PATH`, pass the absolute path as `--command`.

Restart Hermes after editing the config. Hermes registers the tools as `mcp_computer_use_linux_<tool>` and creates the `mcp-computer-use-linux` runtime toolset.

You can verify both sides before asking Hermes to use the desktop:

```bash
computer-use-linux doctor | jq .readiness
hermes skills inspect agent-sh/computer-use-linux/computer-use-linux
hermes chat --toolsets mcp-computer-use-linux -q "List the current desktop windows."
```

For one-off installs without adding the tap first, Hermes also accepts `hermes skills install agent-sh/computer-use-linux/skills/computer-use-linux`.

### Generic MCP client

Spawn the binary with `["mcp"]` as the argv tail. It speaks JSON-RPC over stdio per the rmcp 2024-11-05 protocol; capability discovery happens through `tools/list` and the `doctor` tool. The server normally needs no MCP-specific configuration, but desktop runtime environment still matters (`DBUS_SESSION_BUS_ADDRESS`, `XDG_RUNTIME_DIR`, KDE desktop portals, AT-SPI, and optionally `ydotoold`).

## First-run checklist

1. **Run `doctor`.**

   ```bash
   computer-use-linux doctor | jq .readiness
   ```

   Aim for `can_register_mcp_tools`, `can_build_accessibility_tree`, `can_send_development_input`, `can_query_windows`, and `can_capture_screenshots` all `true`. The `blockers` array should be empty. `can_capture_screenshots` means a route was detected, not that a test capture succeeded.

2. **If AT-SPI is unavailable** — install/configure your distribution's system-level GTK and AT-SPI prerequisites. There is no project setup command for accessibility. Restart target apps if needed.

3. **If `windowing.can_list_windows = false`** — check that KWin scripting is available on the session bus.

4. **Allow portal capture/input when prompted.** XDG desktop portals may request permission before the first screenshot or RemoteDesktop input session.

5. **If using ydotool, confirm `ydotoold` is available.**

   ```bash
   systemctl --user status ydotoold
   ```

   Its socket should appear at `/run/user/$UID/.ydotool_socket`.

## Environment variables

These optional environment variables configure the server or npm wrapper.

**Server runtime** (set in the MCP host's environment):

| Variable | Effect |
| --- | --- |
| `COMPUTER_USE_LINUX_NOTIFY_ON_COMPLETE` | Set exactly to `1` to expose the optional `complete_interaction` notification tool. Requires `notify-send` and a desktop notification service; disabled by default. |
| `CU_DISABLE_ABS_POINTER` | Disable the uinput absolute pointer and click through `ydotool` instead for setups where the abs-pointer device misbehaves. |
| `COMPUTER_USE_LINUX_PERSIST_REMOTE_DESKTOP` | Set exactly to `1` to ask a version 2 or newer RemoteDesktop portal to remember pointer and keyboard grants across processes. The first dialog still appears. Later processes reuse separate single-use restore tokens stored with mode `0600` under `$XDG_STATE_HOME/computer-use-linux/` (or `~/.local/state/computer-use-linux/`). Unset, every new process is prompted. No effect when input is not using the portal. |
| `COMPUTER_USE_LINUX_ENABLE_SHELL` | Set exactly to `1` before starting the MCP server to register the destructive `run_shell` tool. Unset by default. Do not enable for untrusted or unattended MCP hosts. |

**npm wrapper** (set during `npm install`, or before running):

| Variable | Effect |
| --- | --- |
| `COMPUTER_USE_LINUX_BIN` | Run this binary instead of the one bundled by the npm package. |
| `COMPUTER_USE_LINUX_DOWNLOAD_BASE` | Override the GitHub release base URL the installer downloads from (mirrors, air-gapped hosts). |
| `COMPUTER_USE_LINUX_SKIP_DOWNLOAD=1` | Skip the post-install binary download entirely. |
| `COMPUTER_USE_LINUX_LOCAL_BINARY` | Install from a local build instead of downloading (used by CI and local testing). |

## Architecture

- **Accessibility tree** — [`atspi`](https://crates.io/crates/atspi) crate (tokio backend) talks to the AT-SPI registry on the user session bus. The tree is flattened to `(role, name, text, states, bounds)` tuples and indexed; element indices are stable for the duration of a `get_app_state` snapshot.
- **Desktop integration** — [`zbus`](https://crates.io/crates/zbus) for XDG screenshot and RemoteDesktop portal calls and temporary KWin scripting.
- **MCP transport** — [`rmcp`](https://crates.io/crates/rmcp) with the `transport-io` feature; stdio framing, no network.
- **Input** — pointer and keyboard input can use the RemoteDesktop portal; ydotool is an input fallback, with direct uinput available for absolute pointer actions. Text entry supports KDE clipboard typing.
- **Window registry** — `list_windows`, `focused_window`, `activate_window`, `move_window`, and `resize_window` use KWin scripting.
- **Terminal enrichment** — `list_windows` cross-references each terminal window with its controlling TTY and the foreground process on that TTY, so `type_text` / `press_key` can target "the terminal where `pytest` is running" without the host ever knowing the window id.

## Security

Computer-use tooling is, by definition, a privilege-escalation surface. The threat model:

- **`ydotoold` runs as a per-user service** with read/write access to `/dev/uinput`. `install.sh` automates this for systemd user sessions and prints manual supervisor guidance elsewhere. Any process that can connect to its socket (`/run/user/$UID/.ydotool_socket`, mode `0600` by default) can synthesize arbitrary input — keypresses, clicks, anything. Keep the socket in the user runtime dir (the default), not in `/tmp` or any world-readable location. Do not run `ydotoold` as root or as a system service.
- **Desktop portals request permission.** Granting screenshot or RemoteDesktop access lets the MCP host capture or control the desktop for the permitted session. If you don't want screenshot access, decline the prompt and use `get_app_state` with `include_screenshot: false`.
- **AT-SPI exposes window contents to clients on your session bus.** It is also used by screen readers and shares the same trust boundary. Install and configure the system-level GTK/AT-SPI prerequisites required by your applications.
- **Persisted remote control is opt-in.** `COMPUTER_USE_LINUX_PERSIST_REMOTE_DESKTOP=1` stores separate portal restore tokens for pointer and keyboard in the user state directory with mode `0600`. A same-user process that can read those files can restore control without a new prompt until the desktop revokes the grant. Leave the variable unset to keep a prompt on every new process.
- **No network.** This binary opens no TCP/UDP listener, makes no outbound Internet connections, and ships no telemetry. It does use local session transports such as DBus and the per-user `ydotoold` Unix socket.
- **Mutating tools are explicit.** The MCP tool list annotates read-only versus mutating tools, and CI fails if the published tool annotations drift from the table above. Treat those annotations as hints; the host is still responsible for user approval and policy.

If you're running this on a shared workstation, set `ydotoold`'s socket permissions to `0600` (the default) and audit which processes on your user can `connect()` to it.

## Troubleshooting

To receive an explicit completion cue, start the MCP server with
`COMPUTER_USE_LINUX_NOTIFY_ON_COMPLETE=1`. This exposes `complete_interaction`,
a parameter-free tool the agent calls once after finishing its desktop work.
It submits a notification through `notify-send` with a two-second execution
limit and bounded process cleanup. Missing services, errors, or timeouts return
`cue: "skipped"`; notification settings may suppress a submitted cue. This does
not reserve the desktop or prove that other clients have stopped sending input.
No sound or additional desktop settings are enabled by this option.
This option currently applies only to directly spawned MCP hosts. The native Pi
extension does not forward the flag or include this optional tool in its catalog.

`computer-use-linux doctor` is the source of truth. Common failure modes and fixes:

- **AT-SPI is unavailable** — verify your system-level GTK/AT-SPI packages and session accessibility services, then restart target applications if needed. There is no application setup command for accessibility.
- **`windowing.can_list_windows = false`** — check that KWin scripting is available on the session bus.
- **`input.ydotool_socket.ok = false` while using ydotool** — the daemon isn't running. On systemd, run `systemctl --user enable --now ydotoold`. On other init systems, use the per-user supervisor guidance from `./install.sh`.
- **`input.ydotool.ok = false` with an unsupported CLI message** — install ydotool 1.0.3 or newer. A running daemon or socket alone is not enough; `doctor` verifies the required command set before advertising the backend.
- **`input.uinput.ok = false`** — `/dev/uinput` isn't accessible to your user. Configure the device permissions using your distribution's guidance and re-login. Direct uinput provides absolute pointer input; use a portal or ydotool path for keyboard input.
- **Portal calls hang or time out** — check that `xdg-desktop-portal` and its KDE backend are running, then inspect their user-service logs.
- **Screenshots return black frames on multi-monitor setups** — known portal / compositor edge case. Use `get_app_state` with `include_screenshot: false` and rely on AT-SPI until the portal backend is healthy.
- **`type_text` types into the wrong window** — pass an explicit target (`window_id`, `pid`, `wm_class`, `title`, or for terminals `tty` / `terminal_pid` / `terminal_command` / `terminal_cwd`). Without a target, input goes to whatever window currently has compositor focus.
- **Wayland pointer actions miss an unfocused window** — injected pointer input is subject to the compositor's input-focus rules. Call `activate_window` for the target before `click`, `drag`, or coordinate `scroll`; a pointer can land at the requested coordinate without the unfocused surface receiving the action.

If `doctor` is green and a specific tool still misbehaves, file an issue with the JSON output of `doctor` and the failing tool's request payload.

For coordinate calibration, launch [the GTK4 probe](examples/coordinate_probe.py)
in your test desktop and take a targeted screenshot. Choose the center of its
green 10x10 square from that screenshot, convert by `scale`, and click relative
to the same target window. The probe's delivered-event `hit: true` is the
acceptance condition. Do not pass its widget-local `(85, 85)` directly as a
window-relative click: margins and decorations belong to different spaces.

[The semantic-click regression](scripts/semantic_click_test.py) runs against a
built binary in an isolated graphical display and checks GTK button activation
through MCP at scales 1 and 2.

## Related

- [agent-workspace-linux](https://github.com/agent-sh/agent-workspace-linux) — the sibling MCP that gives an agent its **own** isolated Linux desktop (a hidden Xvfb display with its own apps and browser) instead of driving yours. It is the inverse of this project: `computer-use-linux` automates the desktop you are already on; `agent-workspace-linux` sandboxes the agent in a separate one. Use them together.

## Contributing

Contributions are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for the local development workflow, CI gates, and PR expectations. Report security vulnerabilities through [SECURITY.md](SECURITY.md), not public issues.

## Credits

Extracted from [`codex-desktop-linux`](https://github.com/avifenesh/codex-desktop-linux), the Linux distribution of Codex Desktop, which continues to ship this same binary as a bundled plugin. Maintained by [Avi Fenesh](https://github.com/avifenesh).

Built on top of:

- [`atspi`](https://crates.io/crates/atspi) — AT-SPI bindings
- [`zbus`](https://crates.io/crates/zbus) — async DBus
- [`rmcp`](https://crates.io/crates/rmcp) — MCP runtime
- [`ydotool`](https://github.com/ReimuNotMoe/ydotool) — Wayland-friendly uinput driver

## Publishing

Publishing is tag-driven from GitHub Actions. The repository needs these Actions secrets:

```bash
gh secret set CARGO_REGISTRY_TOKEN -R agent-sh/computer-use-linux
gh secret set NPM_TOKEN -R agent-sh/computer-use-linux
```

Then bump `Cargo.toml` and `package.json` together, update `CHANGELOG.md`, and push a `vX.Y.Z` tag. CI runs the full Rust and MCP safety gates, builds release assets for both architectures, publishes `computer-use-linux` to crates.io, and publishes the npm wrapper after the GitHub release binaries are available.

## License

MIT — see [LICENSE](LICENSE).
