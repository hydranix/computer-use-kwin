# KDE Plasma 6 Wayland Lean Build

## Goal

Make `computer-use-linux` a KDE Plasma 6 on Wayland–focused MCP server by
removing desktop integrations and support paths that are not used on that
system, while preserving shared desktop-control functionality.

## Design

The server will retain its shared MCP and AT-SPI functionality, XDG desktop
portal capture and RemoteDesktop input, ydotool, and the direct evdev/uinput
absolute-pointer fallback. KWin scripting remains the sole window-management
backend. It will continue to list, focus, and activate windows, and will gain
KWin-script implementations for `move_window` and `resize_window` so these
existing MCP actions remain functional.

Remove GNOME Shell extension and introspection support, GNOME screenshot and
gsettings-based accessibility setup/guard behavior, COSMIC and its helper,
Hyprland, i3, native X11/EWMH windowing and screenshot support, X11-only
xdotool input, and wtype input. The current wtype compatibility check excludes
KDE/Plasma, so it cannot provide input on the target system. Keep portal
operations available as the desktop-native capture and input paths, with
ydotool and direct uinput as fallbacks.

Remove the corresponding no-longer-applicable MCP tools and CLI commands,
backend diagnostics and capabilities, helper binary/build/package wiring,
and installer setup. Keep diagnostics for the remaining capabilities. Update
current user-facing docs and generated tool descriptions to describe KDE
Plasma 6 Wayland; leave historical changelog entries unchanged.

Dependencies used only by removed backends will be removed after verifying
they have no remaining call sites. Dependencies shared with retained portal,
AT-SPI, or input behavior must remain.

## Alternatives considered

1. **Remove GNOME only:** retains COSMIC, Hyprland, i3, and X11 paths and their
   maintenance and packaging overhead.
2. **KDE-focused build (selected):** removes all non-KDE compositor and X11
   integrations while preserving shared desktop-control capabilities.
3. **Build-time backend profiles:** preserves portability but adds feature,
   build, and packaging complexity instead of simplifying the target build.

## Compatibility and behavior

This intentionally narrows supported environments to KDE Plasma 6 on Wayland.
The doctor report and MCP surface will no longer advertise removed backends or
GNOME setup operations. Existing shared observation and input behavior remains
available. Window move and resize continue to be available through KWin
scripting rather than failing after removal of their old GNOME/X11
implementations.

## Validation

- Rust tests cover remaining portal, ydotool, direct-uinput, and KWin window
  behavior, including KWin move and resize scripts and window geometry.
- CLI and MCP contract tests confirm removed setup commands/tools are absent
  and the remaining diagnostics describe only supported paths.
- Installer, npm packaging, and generated-tool checks confirm the COSMIC
  helper and removed platform integrations are no longer packaged or
  installed.
- Documentation and repository searches verify current setup guidance no
  longer directs KDE users to removed platform-specific integrations.
