# KDE Plasma 6 Wayland Lean Build Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Remove non-KDE desktop integrations from the MCP server while retaining shared portal, AT-SPI, ydotool, direct-uinput, and KWin behavior on KDE Plasma 6 Wayland.

**Architecture:** Keep the existing Rust MCP server and shared desktop primitives, but make KWin the sole window backend and XDG portals the sole screenshot backend. Remove platform-specific modules and their routing, diagnostics, CLI/MCP setup surfaces, installer/package/release wiring, and current documentation. Preserve window geometry tools by implementing them through the existing temporary KWin scripting transaction.

**Tech Stack:** Rust/Tokio, zbus and KWin JavaScript scripting, XDG desktop portals, evdev/uinput, Cargo, Bash, Node.js/TypeScript, Pi tool catalog generation.

---

## File map

- **KWin scripting and routing:** `src/windowing/backends/kwin.rs` owns the temporary script lifecycle and KWin operations. `src/windowing/registry.rs` selects and dispatches window operations. `src/windows.rs` resolves targets and exports the public window API.
- **Screenshots and input:** `src/screenshot.rs` implements portal capture and image payloads. `src/server.rs` routes MCP input and window tools. `src/ydotool.rs`, `src/abs_pointer.rs`, and `src/remote_desktop.rs` remain the input implementations.
- **Diagnostics and CLI:** `src/diagnostics.rs`, `src/cli.rs`, `src/lib.rs`, and `src/accessibility_guard.rs` describe/provide environment checks and setup commands. `src/gnome_extension.rs` provides the GNOME setup tool.
- **Platform modules:** remove `src/windowing/backends/{cosmic,gnome,hyprland,i3,x11}.rs`, `src/cosmic_helper.rs`, `src/bin/computer-use-linux-cosmic.rs`, `src/x11_display.rs`, `src/gnome_extension.rs`, `src/accessibility_guard.rs`, and `gnome-shell-extension/computer-use-linux@avifenesh.dev/{extension.js,metadata.json}` after deleting their references.
- **Build and installation:** `Cargo.toml`/`Cargo.lock`, `install.sh`, `scripts/install_sh_test.sh`, `.github/workflows/ci.yml`, `package.json`, `npm/install.js`, and `npm/bin/computer-use-linux.js` control binary, dependency, system-package, and npm release wiring.
- **Tool surface and docs:** `scripts/mcp_safety_check.py`, `scripts/generate_pi_tool_catalog.py`, `pi/extension/{index.ts,generated-tools.ts}`, Pi tests, `README.md`, `npm/README.md`, and `skills/computer-use-linux/SKILL.md` describe and verify the public surface. Keep `scripts/semantic_click_test.py`; its private GTK/GSettings setup is a test fixture for the shared AT-SPI path, not a server GNOME integration.

### Task 1: Preserve and complete KWin window geometry operations

**Files:**
- Modify: `src/windowing/backends/kwin.rs`
- Modify: `src/windowing/registry.rs`
- Test: KWin unit tests in `src/windowing/backends/kwin.rs`
- Test: registry unit tests in `src/windowing/registry.rs`

- [ ] **Step 1: Add failing KWin script-source tests**

Add unit tests for move and resize script source generation. Assert each script resolves the target window by its normalized UUID, sets `frameGeometry` to the requested geometry, emits a successful result through the registered callback, and reports a missing window or invalid geometry as an error. Test that move preserves current width/height and resize preserves current x/y.

Use these geometry expectations in the test fixture:

```rust
assert!(move_script.contains("window.frameGeometry = Qt.rect(120, 240, frame.width, frame.height)"));
assert!(resize_script.contains("window.frameGeometry = Qt.rect(frame.x, frame.y, 640, 480)"));
```

Run: `cargo test kwin_move_resize_script --lib`

Expected: FAIL because the KWin backend has no move/resize script operation yet.

- [ ] **Step 2: Implement KWin move/resize script operations**

Reuse `kwin_uuid_for_window_id`, `call_kwin_script`, `KwinCallbackKind::Result`, and the existing callback result parsing. Generate scripts that use Plasma 6's `window.frameGeometry` with `Qt.rect(x, y, width, height)`. For move, read the current frame dimensions; for resize, read the current frame origin. Reject non-positive resize dimensions and return an explicit error if the requested window no longer exists or KWin refuses the operation.

Expose:

```rust
pub async fn move_window(window_id: u64, x: i32, y: i32) -> Result<String>;
pub async fn resize_window(window_id: u64, width: i32, height: i32) -> Result<String>;
```

- [ ] **Step 3: Route geometry operations through KWin**

In `src/windowing/registry.rs`, dispatch `KWIN_BACKEND` to the new functions. Keep explicit unsupported-backend errors for any non-KWin `WindowInfo`; after platform cleanup KWin is the only valid backend.

- [ ] **Step 4: Verify geometry operations and existing KWin behavior**

Add a registry test for KWin move/resize dispatch and retain existing KWin list/focus/activate transaction tests.

Run: `cargo test windowing::backends::kwin --lib && cargo test windowing::registry --lib`

Expected: PASS.

- [ ] **Step 5: Commit the KWin geometry change**

```bash
git add src/windowing/backends/kwin.rs src/windowing/registry.rs
git commit -m "feat(kwin): support moving and resizing windows"
```

### Task 2: Make KWin the only windowing backend

**Files:**
- Modify: `src/windowing/registry.rs`
- Modify: `src/windowing/backends/mod.rs`
- Modify: `src/windowing/mod.rs`
- Modify: `src/windows.rs`
- Modify: `src/server.rs`
- Modify: `src/terminal.rs`
- Delete: `src/windowing/backends/cosmic.rs`
- Delete: `src/windowing/backends/gnome.rs`
- Delete: `src/windowing/backends/hyprland.rs`
- Delete: `src/windowing/backends/i3.rs`
- Delete: `src/windowing/backends/x11.rs`
- Test: registry/window tests in the same Rust modules

- [ ] **Step 1: Replace multi-backend registry assertions with KDE-only tests**

In `src/windowing/registry.rs`, test that descriptors contain only `KWIN_BACKEND`, list/focus/activate and geometry dispatch accept KWin windows, and unsupported backend IDs fail explicitly. Remove fallback-order and GNOME/X11 preference tests.

Run: `cargo test windowing::registry --lib`

Expected: FAIL while descriptors and dispatch still reference removed backends.

- [ ] **Step 2: Simplify registry and public window helpers**

Remove `BackendKind` alternatives, backend fallback order, GNOME/X11 special handling, COSMIC focused-window override, and stale permission hints. Keep KWin probe/list/focus/activate and geometry dispatch. Update `src/windows.rs` and `src/server.rs` to remove backend constants, per-backend branches, and GNOME monitor handling while retaining target resolution and existing common window output fields.

- [ ] **Step 3: Remove other compositor modules**

Remove the five non-KWin backend module declarations and their source files listed above. Remove GNOME-only terminal backend tagging in `src/terminal.rs`, but retain generic terminal process and PTY enrichment.

- [ ] **Step 4: Verify windowing and library exports**

Run: `cargo test windowing --lib && cargo test --test library_exports`

Expected: PASS; the only backend exposed by the registry is KWin.

- [ ] **Step 5: Commit KDE-only window routing**

```bash
git add src/windowing src/windows.rs src/server.rs src/terminal.rs
git rm src/windowing/backends/cosmic.rs src/windowing/backends/gnome.rs src/windowing/backends/hyprland.rs src/windowing/backends/i3.rs src/windowing/backends/x11.rs
git commit -m "refactor(windowing): keep only KWin backend"
```

### Task 3: Keep portal screenshots and remove GNOME/X11 capture

**Files:**
- Modify: `src/screenshot.rs`
- Modify: `src/diagnostics.rs`
- Delete: `src/x11_display.rs`
- Test: screenshot tests in `src/screenshot.rs`

- [ ] **Step 1: Add portal-only capture assertions**

Keep the existing portal response/path and payload tests. Replace backend parser tests with assertions that only the portal capture path is selectable and that portal failures surface as errors rather than invoking shell/X11 commands.

Run: `cargo test screenshot --lib`

Expected: FAIL while forced and fallback paths still include removed backends.

- [ ] **Step 2: Remove GNOME Shell, gnome-screenshot, and native-X11 routes**

Keep `capture_with_portal`, screenshot payload conversion, and portal error context. Remove the `ScreenshotBackend` variants and parsers for GNOME/X11, fallback invocations of those methods, `capture_with_x11`, the `x11_display` imports, and unused PNG encoding helpers. Remove the screenshot-backend override environment variable if it only selects the one remaining portal backend.

- [ ] **Step 3: Make diagnostics report portal screenshot capability only**

Remove GNOME screenshot and native-X11 screenshot fields and route-selection logic from diagnostics. Keep screenshot readiness tied to the portal's exported `Screenshot` method.

- [ ] **Step 4: Verify portal and screenshot payload behavior**

Run: `cargo test screenshot --lib && cargo test diagnostics::tests --lib`

Expected: PASS; portal capture remains, with no GNOME/X11 fallback advertised.

- [ ] **Step 5: Commit portal-only screenshot support**

```bash
git add src/screenshot.rs src/diagnostics.rs
git rm src/x11_display.rs
git commit -m "refactor(screenshot): use desktop portal only"
```

### Task 4: Retain portal, ydotool, and direct uinput input paths

**Files:**
- Modify: `src/server.rs`
- Modify: `src/diagnostics.rs`
- Modify: `src/cli.rs`
- Modify: `Cargo.toml`
- Modify: `Cargo.lock`
- Test: input policy tests in `src/server.rs` and `src/diagnostics.rs`
- Delete: `src/bin/computer-use-linux-cosmic.rs`
- Delete: `src/cosmic_helper.rs`

- [ ] **Step 1: Add tests for the supported KDE input order**

Test that portal RemoteDesktop is preferred when available, ydotool remains selectable as fallback or when forced, and the direct absolute-pointer fallback remains advertised when uinput is available. Remove tests for X11 xdotool and wtype selection.

Run: `cargo test input --lib`

Expected: FAIL until obsolete input policy and helper code is removed.

- [ ] **Step 2: Remove xdotool and wtype runtime branches**

Delete X11 XTEST keyboard/pointer selection, wtype selection and launch logic, and their environment overrides. Keep KDE clipboard text handling, portal keysyms, ydotool, and direct uinput pointer behavior. Keep `evdev` because direct-uinput absolute pointer support depends on it. Remove `xkeysym` or other dependencies only if the remaining portal/key mapping code has no references.

- [ ] **Step 3: Remove the COSMIC helper and binary target**

Delete the helper modules and `[[bin]]` target, remove helper environment lookup from `src/server.rs`, `src/cli.rs`, and `src/diagnostics.rs`, and remove `cosmic-protocols` from `Cargo.toml` if no remaining references exist.

- [ ] **Step 4: Reconcile Cargo dependencies**

Regenerate the lockfile with Cargo after removing backend-only dependencies. Keep zbus, Wayland portal, AT-SPI, `evdev`, and libraries used by KWin/portal code.

Run: `cargo check --locked --all-targets`

Expected: PASS without the removed helper target or unused backend dependencies.

- [ ] **Step 5: Commit input and helper removal**

```bash
git add Cargo.toml Cargo.lock src/server.rs src/diagnostics.rs src/cli.rs
git rm src/bin/computer-use-linux-cosmic.rs src/cosmic_helper.rs
git commit -m "refactor(input): remove non-KDE backends"
```

### Task 5: Remove GNOME setup surfaces and narrow diagnostics

**Files:**
- Modify: `src/diagnostics.rs`
- Modify: `src/cli.rs`
- Modify: `src/server.rs`
- Modify: `src/lib.rs`
- Modify: `scripts/mcp_safety_check.py`
- Delete: `src/accessibility_guard.rs`
- Delete: `src/gnome_extension.rs`
- Delete: `scripts/accessibility_guard_test.sh`
- Test: diagnostics unit tests and MCP safety contract

- [ ] **Step 1: Update MCP contract expectations**

Remove `setup_accessibility` and `setup_window_targeting` from expected/ idempotent/open-world tool sets in `scripts/mcp_safety_check.py`; assert those tools are absent. Keep `move_window` and `resize_window` as non-destructive, idempotent window tools.

Run: `scripts/mcp_safety_check.py --binary target/debug/computer-use-linux`

Expected: FAIL until the Rust MCP surface is updated.

- [ ] **Step 2: Remove GNOME setup and guard commands/tools**

Remove the `setup_accessibility` and `setup_window_targeting` MCP tool handlers, CLI `setup`, `setup-window-targeting`, and `guard-accessibility` branches/help text, GNOME setup report exports, module declarations, and imports. Delete the two implementation modules and accessibility guard shell test.

- [ ] **Step 3: Restrict diagnostics structures and readiness logic**

Remove GNOME Shell, COSMIC, Hyprland, i3, X11, and xdotool/wtype report fields and checks. Keep platform/session facts needed for Wayland and portals, AT-SPI checks, KWin availability, ydotool/uinput checks, and portal capabilities. Adjust readiness blockers and recommended actions to name only retained mechanisms; do not silently imply unavailable setup commands.

- [ ] **Step 4: Verify diagnostics and MCP tool contract**

Run: `cargo test diagnostics --lib && cargo build --locked && scripts/mcp_safety_check.py --binary target/debug/computer-use-linux`

Expected: PASS; `doctor` reports only retained backends and removed setup tools are absent.

- [ ] **Step 5: Commit KDE diagnostics and setup-surface changes**

```bash
git add src/diagnostics.rs src/cli.rs src/server.rs src/lib.rs scripts/mcp_safety_check.py
git rm src/accessibility_guard.rs src/gnome_extension.rs scripts/accessibility_guard_test.sh
git commit -m "refactor(diagnostics): remove GNOME setup surfaces"
```

### Task 6: Remove obsolete installer and release packaging

**Files:**
- Modify: `Cargo.toml`
- Modify: `install.sh`
- Modify: `scripts/install_sh_test.sh`
- Modify: `.github/workflows/ci.yml`
- Modify: `npm/install.js`
- Modify: `npm/bin/computer-use-linux.js`
- Modify: `package.json`
- Delete: `gnome-shell-extension/computer-use-linux@avifenesh.dev/extension.js`
- Delete: `gnome-shell-extension/computer-use-linux@avifenesh.dev/metadata.json`

- [ ] **Step 1: Update installer regression tests**

Remove tests that expect X11-only xdotool package installation and add assertions that installer help, dependencies, build outputs, and flags only mention the single server binary, AT-SPI, portal readiness, and optional ydotoold setup.

Run: `bash -n install.sh scripts/install_sh_test.sh && scripts/install_sh_test.sh`

Expected: FAIL until obsolete installer paths are removed.

- [ ] **Step 2: Simplify `install.sh`**

Remove GNOME extension packing/installation, toolkit-accessibility gsettings setup, COSMIC helper installation, X11-specific package dependencies, and related flags/output. Keep distro detection, AT-SPI system packages, optional ydotoold setup, build/install of `computer-use-linux`, and final doctor report.

- [ ] **Step 3: Remove npm COSMIC helper downloads and environment injection**

In `npm/install.js`, download, checksum, and install only the main platform binary. In `npm/bin/computer-use-linux.js`, remove COSMIC helper discovery and environment injection. Remove helper-specific npm package file entries and update CI commands so the local binary path is the only provided binary.

- [ ] **Step 4: Remove helper release artifacts and extension files**

In `.github/workflows/ci.yml`, build, checksum, and upload only the main binary for each release target. Delete the GNOME Shell extension source and metadata. Ensure no install/package flow still expects either removed artifact.

- [ ] **Step 5: Verify installation and packaging**

Run: `bash -n install.sh scripts/install_sh_test.sh && scripts/install_sh_test.sh && node --check npm/install.js && node --check npm/bin/computer-use-linux.js && npm run pack:check`

Expected: PASS; packaged files and release artifacts do not contain the COSMIC helper or GNOME extension.

- [ ] **Step 6: Commit installer and packaging cleanup**

```bash
git add Cargo.toml install.sh scripts/install_sh_test.sh .github/workflows/ci.yml npm/install.js npm/bin/computer-use-linux.js package.json
git rm gnome-shell-extension/computer-use-linux@avifenesh.dev/extension.js gnome-shell-extension/computer-use-linux@avifenesh.dev/metadata.json
git commit -m "build: remove non-KDE platform artifacts"
```

### Task 7: Regenerate tool catalog and update current documentation

**Files:**
- Modify: `README.md`
- Modify: `npm/README.md`
- Modify: `skills/computer-use-linux/SKILL.md`
- Modify: `pi/extension/index.ts`
- Regenerate: `pi/extension/generated-tools.ts`
- Test: Pi tests, tool catalog check, docs/current-reference search

- [ ] **Step 1: Remove removed-tool aliases and helper wiring from the Pi extension**

Delete `setup_accessibility` and `setup_window_targeting` search aliases and COSMIC helper environment/path handling from `pi/extension/index.ts`. Keep window geometry aliases, which remain supported through KWin.

- [ ] **Step 2: Regenerate the checked-in Pi tool catalog**

After building the Rust server, run:

```bash
python3 scripts/generate_pi_tool_catalog.py --binary target/debug/computer-use-linux
```

Expected: `pi/extension/generated-tools.ts` excludes GNOME setup tools and describes KWin/Plasma window geometry.

- [ ] **Step 3: Rewrite current setup and capability guidance**

Update the three user-facing documents to state KDE Plasma 6 Wayland support and explain retained portal, AT-SPI, ydotool, direct-uinput, and KWin requirements. Remove current instructions for GNOME setup, other compositors, X11, wtype, xdotool, and the COSMIC helper. Preserve unrelated historical changelog text.

- [ ] **Step 4: Verify generated tools, extension tests, and current docs**

Run:

```bash
python3 scripts/generate_pi_tool_catalog.py --binary target/debug/computer-use-linux --check
npm test --prefix pi
npm run typecheck:pi
```

Search current docs and active runtime/build source for removed setup commands, helper variables, and backend names; allow matches only in historical changelog entries or tests that assert the obsolete surface is absent. Keep `scripts/semantic_click_test.py` as the isolated AT-SPI integration test.

- [ ] **Step 5: Commit catalog and documentation updates**

```bash
git add README.md npm/README.md skills/computer-use-linux/SKILL.md pi/extension/index.ts pi/extension/generated-tools.ts
git commit -m "docs: document KDE Plasma Wayland support"
```

### Task 8: Run integrated verification

**Files:**
- Verify: all changed Rust, installer, npm, and Pi integration surfaces

- [ ] **Step 1: Run formatting, lint, and Rust tests**

Run: `cargo fmt --all -- --check && cargo clippy --locked --all-targets -- -D warnings && cargo test --locked --all-targets`

Expected: PASS without warnings or failures.

- [ ] **Step 2: Run installer, package, MCP, and Pi checks**

Run:

```bash
bash -n install.sh scripts/install_sh_test.sh
scripts/install_sh_test.sh
scripts/mcp_safety_check.py --binary target/debug/computer-use-linux
npm run pack:check
npm run build:check --prefix pi
npm run typecheck:pi
npm test --prefix pi
```

Expected: PASS; only KWin, portal, AT-SPI, ydotool, and direct-uinput paths remain active.

- [ ] **Step 3: Review final diff and commit any integration fixes**

Run: `git diff --check && git status --short && git --no-pager diff --stat`

Expected: no whitespace errors, no unrelated files, and no remaining current runtime or packaging references to removed desktop integrations.

## Self-review

- **Spec coverage:** Tasks 1–2 preserve all KWin window operations and remove alternate window backends; Task 3 leaves portal-only screenshots; Task 4 retains the approved portal/ydotool/direct-uinput input paths and removes wtype; Task 5 removes GNOME setup and obsolete diagnostics; Task 6 removes helper/install/release wiring; Task 7 updates public docs and generated tools; Task 8 validates the integrated result.
- **Placeholders:** No TODO/TBD implementation steps remain. Dependency deletion is conditional on a source-use check so shared portal/key mapping dependencies are not removed by assumption.
- **Type/API consistency:** KWin geometry APIs take `window_id: u64` with integer coordinates or dimensions and return `Result<String>`, matching the registry's existing geometry dispatch. KWin callback operations reuse the existing `KwinCallbackKind::Result` lifecycle.
