# Contributing to computer-use-kwin

Thanks for helping improve KDE Plasma 6 Wayland desktop control for MCP hosts.

This repository is the separate `hydranix/computer-use-kwin` fork and a
KWin-specific modification of
[`agent-sh/computer-use-linux`](https://github.com/agent-sh/computer-use-linux),
originally developed by [Avi Fenesh](https://github.com/avifenesh). It retains
the original MIT license and copyright attribution; contributions here do not
claim authorship of the upstream work.

## Development Setup

```bash
git clone https://github.com/hydranix/computer-use-kwin.git
cd computer-use-kwin
cargo check --locked
cargo test --locked
```

For npm wrapper work:

```bash
node --check npm/install.js
node --check npm/bin/computer-use-kwin.js
npm pack --dry-run
```

The crate and npm package names are intended for future fork publication.
Until availability is verified, use this source checkout rather than registry
install instructions.

## Before Opening a PR

Run the same gates CI runs:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --no-fail-fast
scripts/install_sh_test.sh
scripts/mcp_safety_check.py
agnix .
```

If you changed release packaging, also run:

```bash
cargo publish --dry-run --locked
npm pack --dry-run
```

## PR Guidelines

- Keep changes focused and explain the desktop/session you tested on.
- Include `computer-use-kwin doctor` output for compositor, portal, or accessibility issues.
- Preserve the MCP safety annotations when adding or changing tools.
- Update `README.md`, `npm/README.md`, and `skills/computer-use-kwin/SKILL.md` when user-facing commands change.
- Use conventional commit prefixes when practical (`fix:`, `feat:`, `docs:`, `chore:`).

## Security

Do not open public issues for vulnerabilities. See [SECURITY.md](SECURITY.md).
