# Security Policy

This policy applies to the separate `hydranix/computer-use-kwin` fork, a
KWin-specific modification of
[`agent-sh/computer-use-linux`](https://github.com/agent-sh/computer-use-linux),
originally developed by [Avi Fenesh](https://github.com/avifenesh). The fork
preserves the original MIT license and copyright attribution; the upstream
project remains separate.

For source-based reproduction, use the root of a clone from
`https://github.com/hydranix/computer-use-kwin.git`.

## Reporting Vulnerabilities

Do not open public issues for security vulnerabilities.

Use GitHub private vulnerability reporting on
[`hydranix/computer-use-kwin`](https://github.com/hydranix/computer-use-kwin),
or contact the fork maintainer directly through GitHub.

## Supported Versions

Until a fork release is verified as available, report issues against the
current source in this repository. After fork releases are available, only the
latest published fork version will be supported with security updates.

## Scope

`computer-use-kwin` can observe and mutate a KDE Plasma 6 Wayland desktop
through MCP, AT-SPI, XDG portals, KWin scripting, and `ydotoold`. Security
reports about unintended desktop access, tool annotation drift, unsafe
defaults, packaging integrity, or release asset verification are in scope.
