# computer-use-kwin

Source and usage notes for the `computer-use-kwin` MCP server and its intended
npm package identity, `@hydranix/computer-use-kwin`. This is a separate,
KWin-specific modification of
[`agent-sh/computer-use-linux`](https://github.com/agent-sh/computer-use-linux),
originally developed by [Avi Fenesh](https://github.com/avifenesh). The
upstream MIT license and copyright attribution are preserved.

The npm registry package and fork release assets are not verified as available.
Use a source build for now:

```bash
git clone https://github.com/hydranix/computer-use-kwin.git
cd computer-use-kwin
cargo build --locked
target/debug/computer-use-kwin doctor
```

The server uses XDG desktop portals for capture and RemoteDesktop input,
KWin scripting as its sole window-management backend, and shared AT-SPI for
application state. Input can also use `ydotool`, direct uinput
absolute-pointer events, and KDE clipboard typing.

Ensure the system-level GTK/AT-SPI prerequisites required by your applications
are installed and configured. There is no application accessibility setup
command. Run `ydotoold` as a per-user service only when using the ydotool input
fallback.

Plain left `click` by element index or selector prefers a native AT-SPI
`click`, `press`, or `toggle` action over toolkit bounds, avoiding pointer
coordinate conversion when that action is available. Explicit `activate` and
`jump` requests belong in `perform_action`, not `click`, even when bounds are
missing. Explicit `x`/`y`, right clicks, and multi-clicks retain pointer
semantics. For coordinate `click` or `scroll` with `relative: true`, use the
clipped target-window screenshot crop origin and divide preview coordinates by
screenshot `scale`. A window target is required.

For an optional MCP completion notification, set
`COMPUTER_USE_KWIN_NOTIFY_ON_COMPLETE=1` in the server environment and have
the agent call `complete_interaction` after its desktop work. `notify-send`
must be installed with an available desktop notification service. The cue is
best effort and does not provide exclusive desktop ownership. It is available
only to directly spawned MCP hosts; the native Pi extension does not forward
the flag or include the tool in its catalog.

On Wayland, set `COMPUTER_USE_KWIN_PERSIST_REMOTE_DESKTOP=1` in the server
environment to ask a version 2 or newer RemoteDesktop portal to reuse its
single-use restore tokens across processes. The first dialog still appears;
leave the variable unset to request access for each new process. The repository
README records where the tokens are stored and who can read them.

The generated Hermes config should look like this:

```yaml
mcp_servers:
  computer-use-kwin:
    command: computer-use-kwin
    args: ["mcp"]
    timeout: 120
    connect_timeout: 30
```

After the npm package and fork release assets are published and verified, the
wrapper is intended to download the matching Linux x86_64 or aarch64 binary
and verify its `.sha256` asset before installing it.

After publication, installing through Pi will supply native, dynamically loaded
`computer_use_kwin_*` tools. No separate MCP adapter or manual MCP
configuration is required. Native tools require Pi 0.84.4 or newer; the
standalone CLI wrapper retains Node.js 18 support.

When running the wrapper from a local package build, if you already built or
installed the binary yourself, set
`COMPUTER_USE_KWIN_BIN=/path/to/computer-use-kwin` to make the wrapper use
that executable instead.
