# computer-use-kwin Fork

## Goal

Create a separately maintained, KWin-specific fork named
`computer-use-kwin`, preserving `agent-sh/computer-use-linux` as the original
project and upstream.

## Design

Create the GitHub fork at `hydranix/computer-use-kwin`. Preserve the existing
local commit history, merge the source repository's two newer `main` commits
without rewriting history, and configure the local checkout with the fork as
`origin` and `agent-sh/computer-use-linux` as `upstream`. Keep the current
checkout path unchanged.

Rename active product identifiers consistently: the Rust crate, executable,
and CLI become `computer-use-kwin`; the npm package becomes
`@hydranix/computer-use-kwin`. Update the Pi and Hermes integration identifiers,
skill paths, environment variables, release asset names, workflows, tests, and
current installation and setup documentation. Keep the product scope explicit:
KWin-specific desktop control on KDE Plasma Wayland.

Add a clear README provenance statement identifying this project as a
KWin-specific modification of `computer-use-linux`, originally developed by
Avi Fenesh, with a link to the upstream repository. Retain existing license,
author, and copyright attribution. Do not present the fork as the upstream
project or imply authorship of the original work.

## Alternatives considered

1. **Rename the existing source repository:** rejected because it would change
   the original project's identity instead of preserving it as upstream.
2. **Create a separate repository from the local branch:** rejected because it
   would not retain GitHub's formal fork relationship.
3. **Create a formal fork and merge upstream changes (selected):** preserves
   upstream identity and provenance while retaining the local KWin-focused
   work and integrating the two newer source commits.

## Compatibility

No compatibility packages or old-name release bridges will be maintained.
Previously published `computer-use-linux` packages and releases remain
unchanged; users of this fork must migrate to the new crate, npm package, CLI,
and integration identifiers. Preserve historical changelog references and
upstream URLs, and add a current migration note. Do not publish packages or
release artifacts as part of the rename; first verify registry namespace
ownership and availability.

## Validation

- Merge the source changes without losing local commits; inspect and resolve
  any conflicts.
- Run the Rust checks and tests, installer tests, npm packaging checks, and Pi
  extension tests/type checks.
- Verify release and install configuration consistently uses the new active
  names.
- Search for stale active identifiers, allowing historical changelog entries
  and explicit provenance/upstream references.
