---
name: pi-setup
description: "Pi coding agent setup for native computer-use-kwin tools."
---

# Pi Setup

## Install

Npm package availability has not been verified. Use this source checkout
installation now; the registry instructions below are for a future
publication:

```bash
cargo build --locked
COMPUTER_USE_KWIN_BIN="$PWD/target/debug/computer-use-kwin" \
  pi -e "$PWD/pi/extension/index.ts"
```

After publication, install the package with
`pi install npm:@hydranix/computer-use-kwin`. Restart Pi or run `/reload`. No
separate MCP adapter or MCP configuration is required. The extension does not
put `computer-use-kwin` on `PATH`.

The integration is designed for current Pi releases with additive dynamic tool
loading. Update Pi and installed packages when needed:

```bash
pi update --all
```

## How tool loading works

Pi initially sees one small loader:

```text
computer_use_kwin_tools
```

Enable exact tools:

```text
computer_use_kwin_tools({ tools: ["doctor", "list_windows"] })
```

Or search by capability:

```text
computer_use_kwin_tools({ query: "inspect a window and type text" })
```

The selected native tools appear starting on the next model turn with their
full upstream schemas and remain active for the session:

```text
computer_use_kwin_doctor({})
computer_use_kwin_list_windows({})
computer_use_kwin_type_text({ text: "hello", title: "Notes" })
```

Pi does not start the desktop process when the loader runs. The first real tool
call starts one computer-use-kwin process, and the package reuses it for the
session so `get_app_state` element indices and portal sessions remain valid.

## Safe operating loop

1. Enable `get_app_state`, `list_windows`, and any likely action tools.
2. Call `computer_use_kwin_get_app_state` scoped to the target app
   (`app_name_or_bundle_identifier`, or `window_id`/`pid`/`app_id`/`wm_class`/
   `title`), using `include_screenshot: false` when accessibility data is
   enough. An unscoped call returns the whole desktop tree, reports
   `tree_scoped: false`, and warns; that can exhaust a small context window.
3. Inspect the returned readiness block; enable/call `doctor` only for full
   diagnostics.
4. Identify the target with `computer_use_kwin_list_windows` or
   `computer_use_kwin_focused_window`.
5. Enable and call the required action tool.
6. Re-observe after the UI changes.

Native Computer Use tools execute sequentially. Cancellation is forwarded to
the MCP request. If the server process exits, the failed call is never replayed
automatically; call `get_app_state` again before another element-based action.

## Migration from the adapter-based package

The previous upstream adapter-based integration required `pi-mcp-adapter` and
wrote a `computer-use-linux` entry into `mcp.json` under the configured Pi
agent directory. The native fork keeps reading this old entry only to show a
migration notice.

The native integration does not write MCP configuration. It reads only the
legacy entry location to show a migration notice. After updating:

1. Remove only the `computer-use-linux` entry from the Pi agent `mcp.json` if
   it is still present.
2. Keep `pi-mcp-adapter` if you use it for other MCP servers; otherwise remove
   it with `pi remove npm:pi-mcp-adapter`.
3. Run `/reload`.

The package reports a non-destructive migration notice when it detects the
legacy entry.

## If the binary is not found

The extension looks for `computer-use-kwin` in this order:

1. `COMPUTER_USE_KWIN_BIN`
2. The binary downloaded inside the installed npm package
3. `computer-use-kwin` on `PATH`, from a source build or a future npm/Cargo
   package install

A registry extension (`pi -e npm:@hydranix/computer-use-kwin`) is staged
without the downloaded binary, so it relies on the `PATH` step. The extension
still checks that the server version and tool catalog match, so a mismatched
install fails with that reason instead of starting.

After publication, reinstall the package if the bundled binary is missing:

```bash
pi remove npm:@hydranix/computer-use-kwin
pi install npm:@hydranix/computer-use-kwin
```

Users with a separate build can set:

```bash
export COMPUTER_USE_KWIN_BIN=/absolute/path/to/computer-use-kwin
```

## Verification

Enable and call the readiness tool:

```text
computer_use_kwin_tools({ tools: ["doctor"] })
computer_use_kwin_doctor({})
```

Ready output has:

- `can_register_mcp_tools: true`
- `can_build_accessibility_tree: true`
- `can_query_windows: true`
- `can_send_development_input: true`
- `can_capture_screenshots: true`
- `blockers: []`
