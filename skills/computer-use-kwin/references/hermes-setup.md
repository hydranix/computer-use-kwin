---
name: hermes-setup
description: "Hermes agent setup for the computer-use-kwin MCP server."
---

# Hermes Setup

Registry package availability has not been verified. Build the MCP server
from a source checkout before configuring Hermes:

```bash
cargo build --locked
target/debug/computer-use-kwin doctor
```

Add the server with the Hermes MCP CLI:

```bash
hermes mcp add computer-use-kwin --command computer-use-kwin --args mcp
hermes mcp test computer-use-kwin
hermes mcp configure computer-use-kwin
```

`configure` opens Hermes' tool-selection UI for this MCP server.

The generated config should look like this:

```yaml
mcp_servers:
  computer-use-kwin:
    command: computer-use-kwin
    args: ["mcp"]
    timeout: 120
    connect_timeout: 30
```

If the binary is not on `PATH`, pass the absolute path to `--command`, for
example `"$PWD/target/debug/computer-use-kwin"`. The package install command
can be used after a future npm publication.

Hermes registers tools using the `mcp_<server>_<tool>` pattern. With this config, tool names are prefixed as `mcp_computer_use_kwin_`, for example:

| MCP tool | Hermes tool name |
| --- | --- |
| `doctor` | `mcp_computer_use_kwin_doctor` |
| `get_app_state` | `mcp_computer_use_kwin_get_app_state` |
| `list_windows` | `mcp_computer_use_kwin_list_windows` |
| `click` | `mcp_computer_use_kwin_click` |
| `type_text` | `mcp_computer_use_kwin_type_text` |

Restart Hermes after changing MCP config.
