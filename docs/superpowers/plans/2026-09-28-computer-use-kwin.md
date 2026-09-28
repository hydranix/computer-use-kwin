# computer-use-kwin Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create a formal `hydranix/computer-use-kwin` fork and rename this KWin-specific project consistently without changing the original project or losing either line of history.

**Architecture:** Fork `agent-sh/computer-use-linux` in GitHub, preserve this checkout's local commits, and merge the two newer source commits without rewriting history. Rename the Rust, npm, CLI, Pi/Hermes, environment-variable, release, and documentation identifiers as one coordinated change; retain explicit upstream attribution and historical release records.

**Tech Stack:** Git/GitHub CLI, Rust/Cargo, Node.js/npm, TypeScript/Vitest, Python and Bash CI scripts.

---

## File map

- Rust identity and runtime names: `Cargo.toml`, `Cargo.lock`, `src/main.rs`, `src/cli.rs`, `src/server.rs`, `src/abs_pointer.rs`, `src/remote_desktop.rs`, `src/screenshot.rs`, `src/windowing/backends/kwin.rs`, `src/ydotool.rs`, `src/command_runner.rs`, `src/diagnostics.rs`.
- Rust and MCP contract tests: `tests/cli_surface.rs`, `tests/library_exports.rs`, `scripts/mcp_safety_check.py`, `scripts/zod-check/check.mjs`.
- npm and installer surface: `package.json`, `npm/bin/computer-use-linux.js` (rename to `npm/bin/computer-use-kwin.js`), `npm/install.js`, `npm/README.md`, `install.sh`, `.gitignore`, `scripts/check_npm_package.mjs`, `scripts/install_sh_test.sh`, `scripts/zod-check/package.json`, `scripts/zod-check/package-lock.json`.
- Pi extension: `pi/package.json`, `pi/package-lock.json`, `pi/extension/index.ts`, `pi/extension/mcp-client.ts`, generated `pi/extension/generated-tools.ts` and `pi/extension/mcp-client.bundle.cjs`, `pi/test/index.test.ts`, `pi/test/pi-integration.test.ts`, `pi/test/shell-environment.integration.test.ts`, `pi/test/shell-extension.test.ts`, `pi/test/bundle-smoke.test.ts`, and `pi/test/fixtures/mcp-server.mjs`.
- Skills and current documentation: rename `skills/computer-use-linux/` to `skills/computer-use-kwin/`, including `SKILL.md`, `references/hermes-setup.md`, and `references/pi-setup.md`; update `README.md`, `CONTRIBUTING.md`, and `SECURITY.md`.
- Release automation: `.github/workflows/ci.yml`.
- Preserve `LICENSE`, `CHANGELOG.md`, the existing 2026-09-27 design/plan documents, and explicit upstream references, including the Codex Desktop sync reminder in `.github/workflows/sync-reminder.yml`. Do not publish registry packages or release artifacts.

### Task 1: Creating the formal GitHub fork

**Files:** Git remotes and GitHub repository metadata only.

- [ ] **Step 1: Verify the fork target is still available**

Run:

```bash
gh api user --jq .login
gh repo view hydranix/computer-use-kwin --json name,url
```

Expected: the authenticated owner is `hydranix`; the repository lookup reports that the target does not exist. If either result differs, stop and confirm the target before creating a repository.

- [ ] **Step 2: Create the formal fork and remotes**

Run:

```bash
gh repo fork agent-sh/computer-use-linux \
  --fork-name computer-use-kwin \
  --clone=false \
  --remote
```

Expected: GitHub creates `hydranix/computer-use-kwin`, `origin` points to that fork, and `upstream` points to `agent-sh/computer-use-linux`.

- [ ] **Step 3: Verify fork ancestry and remotes**

Run:

```bash
gh api repos/hydranix/computer-use-kwin --jq '[.full_name, .parent.full_name]'
git remote -v
```

Expected: the API reports `hydranix/computer-use-kwin` with parent `agent-sh/computer-use-linux`; the remotes distinguish `origin` from `upstream`.

### Task 2: Integrating newer upstream history

**Files:** Git history only; do not rewrite or discard commits.

- [ ] **Step 1: Fetch the source branch**

Run:

```bash
git fetch upstream
git log --oneline --left-right upstream/main...HEAD
```

Expected: local KWin work remains present and `upstream/main` includes the two source commits that were absent at planning time. Re-evaluate the divergence from the fetched refs before merging.

- [ ] **Step 2: Merge upstream without rewriting local history**

Run:

```bash
git merge --no-edit upstream/main
```

Expected: a merge commit joins the two histories. If conflicts occur, preserve both the newer upstream behavior and the local KWin-only behavior; do not use reset, force-push, or rebase. If preserving both requires a behavior decision that cannot be established from code/tests, stop and ask.

- [ ] **Step 3: Verify both lines of history remain reachable**

Run:

```bash
git merge-base --is-ancestor upstream/main HEAD
git log --oneline --decorate -8
```

Expected: the ancestor check exits 0 and the recent history includes the upstream merge and local KWin commits.

### Task 3: Renaming the Rust crate, executable, and MCP server identity

**Files:** Rust identity/runtime paths and contract tests listed in the file map.

- [ ] **Step 1: Update CLI and library references in tests first**

In `tests/cli_surface.rs`, change the Cargo executable lookup to
`env!("CARGO_BIN_EXE_computer-use-kwin")` and update the invocation error text
to `run computer-use-kwin CLI`. In `tests/library_exports.rs`, change the
library import to `computer_use_kwin::`. Update MCP safety expectations in
`scripts/mcp_safety_check.py` to require server name `computer-use-kwin`.

- [ ] **Step 2: Verify the rename tests fail before implementation**

Run:

```bash
cargo test --test cli_surface --test library_exports
```

Expected: compilation fails because Cargo does not yet expose the new binary or crate name.

- [ ] **Step 3: Rename Cargo and runtime identifiers**

In `Cargo.toml`, set the package and binary name to `computer-use-kwin`; set
homepage, repository, and documentation URLs to the new fork; preserve the
existing `Avi Fenesh` author entry and MIT license. Update `src/main.rs` to
call `computer_use_kwin::run_cli_from_env()`. Rename active CLI help, MCP
server name, diagnostic/log names, internal temporary prefixes, and
`COMPUTER_USE_LINUX_*` environment variables in the listed Rust files and
`scripts/mcp_safety_check.py` to their `computer-use-kwin`,
`computer_use_kwin`, or `COMPUTER_USE_KWIN_*` equivalents as appropriate.
Update `scripts/zod-check/check.mjs` to invoke the new binary path.

- [ ] **Step 4: Regenerate the lockfile and verify Rust behavior**

Run:

```bash
cargo check --all-targets
cargo test --test cli_surface --test library_exports
```

Expected: Cargo records the renamed root crate in `Cargo.lock`; both targeted test binaries compile and pass. Then run `cargo test --locked --no-fail-fast`.

- [ ] **Step 5: Commit the Rust identity change**

```bash
git add Cargo.toml Cargo.lock src tests scripts/mcp_safety_check.py scripts/zod-check/check.mjs
git commit -m "refactor: rename Rust project to computer-use-kwin"
```

### Task 4: Renaming the npm wrapper and installer

**Files:** npm/installer paths and packaging tests listed in the file map.

- [ ] **Step 1: Add npm package identity and executable assertions**

In `scripts/check_npm_package.mjs`, after parsing `npm pack --dry-run --json`,
assert `pack.name === "@hydranix/computer-use-kwin"` and throw an error that
prints the actual and expected names when they differ. Add
`npm/bin/computer-use-kwin.js` to `requiredFiles` so the packaged executable
entry is checked along with the Pi integration files.

- [ ] **Step 2: Verify the new assertion fails**

Run:

```bash
node scripts/check_npm_package.mjs
```

Expected: it reports the current package name is not `@hydranix/computer-use-kwin`.

- [ ] **Step 3: Rename npm metadata, executable wrapper, and installer names**

Set `package.json` name to `@hydranix/computer-use-kwin`, its `bin` key and
`test:wrapper` script to `computer-use-kwin`, and its wrapper file list to
`npm/bin/computer-use-kwin.js`. Rename `npm/bin/computer-use-linux.js` to
`npm/bin/computer-use-kwin.js`; update its binary lookup and errors. Update
`npm/install.js`, `npm/README.md`, `install.sh`, `.gitignore`, and
`scripts/install_sh_test.sh` so installed binaries, release assets, local
overrides, logs, temp directories, and test fixtures use the new active name
and `COMPUTER_USE_KWIN_*` environment prefix. Rename the zod-check dev package
to `computer-use-kwin-zod-check` in its manifest and lockfile.

- [ ] **Step 4: Verify npm packaging and installer behavior**

Run:

```bash
node --check npm/install.js
node --check npm/bin/computer-use-kwin.js
node scripts/check_npm_package.mjs
bash -n install.sh scripts/install_sh_test.sh
scripts/install_sh_test.sh
npm install --package-lock-only --ignore-scripts --prefix scripts/zod-check
```

Expected: syntax checks and installer regressions pass; package check reports
`@hydranix/computer-use-kwin` and the Pi files remain in the tarball.

- [ ] **Step 5: Commit the wrapper and installer rename**

```bash
git add package.json npm scripts/check_npm_package.mjs scripts/install_sh_test.sh install.sh .gitignore
git add scripts/zod-check/package.json scripts/zod-check/package-lock.json
git commit -m "refactor: rename npm package and installer to computer-use-kwin"
```

### Task 5: Renaming Pi and Hermes integrations

**Files:** Pi extension, tests, generated catalog/bundle, and skill directory listed in the file map.

- [ ] **Step 1: Update Pi tests to assert the new public identifiers**

In Pi tests and `pi/test/fixtures/mcp-server.mjs`, update expected package,
binary, environment, MCP server, loader, and tool-prefix values to
`@hydranix/computer-use-kwin`, `computer-use-kwin`,
`COMPUTER_USE_KWIN_*`, `computer_use_kwin`, and
`computer_use_kwin_<tool>`. Keep assertions that exercise shell isolation and
packaged-binary startup.

- [ ] **Step 2: Run focused Pi tests to verify they fail**

Run:

```bash
npm test --prefix pi -- index.test.ts shell-extension.test.ts
```

Expected: tests fail on old extension/package identifiers before the extension implementation is renamed.

- [ ] **Step 3: Rename Pi integration code and skill paths**

Update `pi/extension/index.ts` and `pi/extension/mcp-client.ts` to use
`@hydranix/computer-use-kwin`, executable `computer-use-kwin`, new loader/tool
prefixes, server identity, and `COMPUTER_USE_KWIN_*` variables. Rename
`skills/computer-use-linux/` to `skills/computer-use-kwin/` and update its
metadata, install commands, tool examples, Hermes configuration, and Pi
configuration. Update `package.json` package-file and skill paths,
`pi/package.json`, and `scripts/generate_pi_tool_catalog.py` to use the new
names.

- [ ] **Step 4: Regenerate generated Pi assets and package lock**

Run:

```bash
npm install --package-lock-only --ignore-scripts --prefix pi
cargo build
python3 scripts/generate_pi_tool_catalog.py --binary target/debug/computer-use-kwin
npm ci --prefix pi --ignore-scripts --no-audit --no-fund
npm run build:pi
```

Expected: npm updates `pi/package-lock.json`; the generated tool catalog and
MCP bundle reflect the new server, executable, and tool prefixes.

- [ ] **Step 5: Verify Pi tests, types, and generated files**

Run:

```bash
npm run check:pi-bundle
npm run typecheck:pi
npm test --prefix pi
python3 scripts/generate_pi_tool_catalog.py --binary target/debug/computer-use-kwin --check
```

Expected: generated files are current, types pass, and all Pi tests pass.

- [ ] **Step 6: Commit the Pi and skill rename**

```bash
git add pi skills package.json scripts/generate_pi_tool_catalog.py
git commit -m "refactor: rename Pi and Hermes integrations"
```

### Task 6: Updating project identity, attribution, and CI/release metadata

**Files:** `README.md`, `npm/README.md`, `CONTRIBUTING.md`, `SECURITY.md`, `.github/workflows/ci.yml`, and current integration guides under `skills/computer-use-kwin/`.

- [ ] **Step 1: Add explicit fork provenance and migration guidance**

Update `README.md` with the new title, badges and links for
`hydranix/computer-use-kwin`, package/crate installation names, KWin-specific
scope, and a migration note. Remove crates.io/npm version badges and latest
release claims until the fork has published packages/releases. Until then,
label registry install commands in these guides as future instructions and
give working source-based build/install commands instead. Include a direct
statement that the project is a
KWin-specific modification of
[`computer-use-linux`](https://github.com/agent-sh/computer-use-linux),
originally developed by Avi Fenesh; preserve the MIT license and author
attribution. Update contribution and security guidance to identify the fork
and its new paths.

- [ ] **Step 2: Update CI and release artifact paths**

In `.github/workflows/ci.yml`, update build/test binary paths, wrapper checks,
local install smoke tests, Pi package path, MCP catalog checks, release asset
names, GitHub release URL, and published executable path to the new identity.
Keep npm publication on `@hydranix/computer-use-kwin`; do not add a publishing
step or trigger a release.

- [ ] **Step 3: Audit active identifiers without rewriting history**

Run:

```bash
rg -n 'computer-use-linux|computer_use_linux|@agent-sh/computer-use-linux|COMPUTER_USE_LINUX' \
  --glob '!CHANGELOG.md' --glob '!docs/superpowers/specs/2026-09-27-*' \
  --glob '!docs/superpowers/plans/2026-09-27-*' \
  --glob '!docs/superpowers/plans/2026-09-28-computer-use-kwin.md' .
```

Review every result. Remaining occurrences must be intentional upstream
provenance or compatibility-history descriptions; update any active command,
package, environment variable, tool, release, or install reference. Keep
`CHANGELOG.md` and the 2026-09-27 historical spec/plan unchanged.
Keep `.github/workflows/sync-reminder.yml` source-specific; it describes the
original project's Codex Desktop embedding rather than the fork's product ID.

- [ ] **Step 4: Commit documentation and CI changes**

```bash
git add README.md CONTRIBUTING.md SECURITY.md .github/workflows/ci.yml
git add npm/README.md skills/computer-use-kwin/SKILL.md
git add skills/computer-use-kwin/references
git add docs/superpowers/plans/2026-09-28-computer-use-kwin.md
git commit -m "docs: identify computer-use-kwin as a KWin-specific fork"
```

### Task 7: Running final gates and publishing the fork branch

**Files:** No additional source files.

- [ ] **Step 1: Run Rust and packaging gates**

Run:

```bash
cargo fmt --all -- --check
cargo check --locked --all-targets
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --no-fail-fast
cargo build --locked
scripts/mcp_safety_check.py --binary target/debug/computer-use-kwin
node scripts/check_npm_package.mjs
COMPUTER_USE_KWIN_BIN="$PWD/target/debug/computer-use-kwin" npm run test:wrapper
```

Expected: every command succeeds; MCP safety checks report server name `computer-use-kwin`.

- [ ] **Step 2: Run installer and Pi gates**

Run:

```bash
bash -n install.sh scripts/install_sh_test.sh
scripts/install_sh_test.sh
npm run build:pi
npm run check:pi-bundle
npm run typecheck:pi
COMPUTER_USE_KWIN_TEST_BINARY="$PWD/target/debug/computer-use-kwin" npm test --prefix pi
python3 scripts/generate_pi_tool_catalog.py --binary target/debug/computer-use-kwin --check
```

Expected: all commands succeed with generated Pi artifacts current.

- [ ] **Step 3: Verify names, attribution, and repository state**

Run:

```bash
git remote -v
gh api repos/hydranix/computer-use-kwin --jq '[.full_name, .parent.full_name]'
git status --short
```

Expected: `origin` is the fork, `upstream` is `agent-sh/computer-use-linux`,
GitHub reports the source as parent, and the worktree has no uncommitted files.
Confirm that old-name occurrences are limited to explicit provenance or
historical records. Do not run `npm publish`, `cargo publish`, or create a
release.

- [ ] **Step 4: Push the completed branch to the fork**

Run:

```bash
git push -u origin main
```

Expected: push succeeds without force; the fork's `main` contains the merged
upstream history and the complete KWin-specific rename.
