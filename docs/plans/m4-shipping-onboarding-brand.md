# M4 plan: logo, first-run setup, installer and GitHub release

Nothing here is built yet except the logo files. Items marked **(you)** need a decision or an account from the owner.

## 1. Logo and brand

Files: `assets/brand/moss-logo.svg` (full colour app icon), `assets/brand/moss-mark-mono.svg` (`currentColor`, for the sidebar, README, tray).

- Idea (v2, after the owner disliked the leaf): a friendly moss mound with two eyes on a blue tile. The mound's two soft peaks also read as an "M". Colours come from the app (blue Compile button, near-black chrome): tile `#4a79ff` to `#2447d6`, mound `#f3f6ff` to `#c3d1ff`, eyes `#2a3a8f` to `#101a52`. A lighter back swell adds the layered look of the owner's references.
- Still to do:
  - [ ] Check the mark at 16, 24 and 32 px (stroke may need thickening for the 16 px icon; the mono file already uses a thicker stroke).
  - [ ] Wordmark file (the "Moss" text converted to paths, no font dependency) for the README banner.
  - [ ] Generate app icons: `npx tauri icon assets/brand/moss-logo.svg` (overwrites `src-tauri/icons/*`; run only with the owner's yes).
  - [ ] Use the mono mark in the sidebar header and the first-run screen; add `ui/app/icon.svg` as the favicon.
  - [ ] Social preview image 1280x640 for the GitHub repo.

## 2. First-run setup ("Set up LaTeX in one click")

Goal: a new user with nothing installed gets to a compiled PDF without leaving Moss. Setup is never forced: editing works without TeX, and the Compile button shows "Set up LaTeX" when TeX is missing. The same screen is reachable later from Settings > Setup.

### Steps (a dialog in the Add-project style, shown once when `onboarded` is false in `settings.json`)
1. **Welcome**: where Projects live (default `Documents\Moss\projects`, changeable), nothing else.
2. **Check this PC** (read-only, uses `tex::detect`): a checklist with a status each for TeX distribution (MiKTeX / TeX Live and version), `latexmk`, Perl, `synctex`, `pdftoppm` (page images for agents), and whether MiKTeX auto-installs missing packages. Everything found: skip to step 4.
3. **Install** (one button, shows real download size, progress per part, Cancel):
   - MiKTeX, per-user (no admin prompt) via the official MiKTeX setup program, basic package set.
   - Perl for `latexmk` (portable, into `%LOCALAPPDATA%\Moss\perl`, no PATH change; Moss stores the folder as its custom TeX bin dir/Perl path instead).
   - Turn on MiKTeX on-the-fly package install and refresh its package database.
   - Optional checkbox "Pre-install common packages" (amsmath, biblatex, pgf/tikz, hyperref, geometry, IEEEtran, beamer, microtype, siunitx, cleveref, and what the 8 Templates use) so the first compile does not stall on downloads.
   - TeX Live users are never installed over; Moss only reports what is missing.
4. **Test project**: creates "Moss tour" from a tour Template (a short document using maths, a figure, a citation and a reference, with comments pointing at the editor features), compiles it, shows the PDF and the timing. On failure it shows the exact reason and a fix button (e.g. "Perl missing, install it").
5. **Optional, skippable**: Connect Overleaf; Connect an AI agent (the existing MCP page's one-click config for Claude Code / Desktop / Cursor / Codex).
6. **Done**: opens the tour Project.

### Backend (`moss-core::setup`)
- `check() -> SetupReport` (pure detection, fast).
- `install(parts, progress_cb, cancel)`: download with HTTPS only, **SHA-256 checked against hashes pinned in the app**, resumable, extracted/run from a temp folder under `%LOCALAPPDATA%\Moss\setup`, log file kept for support. A hash mismatch aborts and deletes nothing else.
- `warm_packages(list, progress_cb)`, `self_test()` (creates the tour Project in a temp folder, compiles, reports).
- Tauri commands + events: `setup_check`, `setup_install`, `setup_cancel`, `setup_self_test`; UI `ui/components/moss/setup-dialog.tsx`.
- Must verify at build time (not checked yet): the current official MiKTeX setup download URL and its silent/per-user flags; which Perl distribution to use (Strawberry Perl portable is the likely choice) and its size and licence; MiKTeX's package-manager command lines.
- Tests: detection against fake folders; download against a local HTTP server (success, hash mismatch, cancel, resume); the install runner with a stub installer. Real run on a clean machine: Windows Sandbox (no TeX installed), recorded in the phase notes.
- ADR 0006 when built: "Moss installs MiKTeX per-user and Perl portably, hash-pinned" (supersedes the last sentence of ADR 0003 in detail, not in spirit).

## 3. Installer and updates

- Bundle target `nsis`, install mode `currentUser` (no admin), WebView2 bootstrapper set to download on demand; optional MSI later.
- `moss-mcp.exe` ships next to `Moss.exe` (the app already looks there, `src-tauri/src/agents.rs`): add it as an `externalBin`/resource and build it in the release workflow before bundling.
- Versions: one number, kept equal in `Cargo.toml` (workspace), `package.json`, `src-tauri/tauri.conf.json`; a small `scripts/bump-version` to change all three; tag `vX.Y.Z`.
- In-app updates with `tauri-plugin-updater`: signing key from `tauri signer generate`; public key in `tauri.conf.json`; private key + password as GitHub secrets `TAURI_SIGNING_PRIVATE_KEY(_PASSWORD)`; endpoint `https://github.com/<owner>/<repo>/releases/latest/download/latest.json`. Back the private key up somewhere safe: losing it strands existing installs. **(you)**
- Code signing (decided: owner said "you decide"): ship the first betas **unsigned** with a README note about SmartScreen's "unknown publisher" warning (More info > Run anyway), and apply to SignPath Foundation (free for open source) in parallel; switch the release workflow to signed builds once approved. Updater signatures (section above) are separate and are on from the first release.

## 4. GitHub

Repo preparation (all before the first commit; nothing is committed yet, and committing needs the owner's yes):
- [ ] Repo name clash: an old `moss` repo (the web version) already exists. Recommended: rename the old one to `moss-web`, archive it with a README line pointing to the new app, then create the new `moss`. (GitHub redirects old links only until a new repo takes the name.) Alternative: new repo `moss-desktop` and leave the old one alone. **(you)** pick, and say public or private.
- [ ] Add `LICENSE` (MIT is declared in `Cargo.toml` but there is no file yet), `README.md` (logo, 3 screenshots, install, build, "unofficial Overleaf sync, not affiliated with Overleaf" and the session-cookie handling), `CONTRIBUTING.md`, `SECURITY.md`, `CHANGELOG.md`, issue/PR templates.
- [ ] Check what would be committed: `.gitignore` already excludes `target`, `.next`, `out`, `.moss/`, `.env*`, `private-fixtures/`, vendor copies; confirm no cookies, tokens, or personal LaTeX documents are tracked; keep `Cargo.lock` and `package-lock.json`.
- [ ] Decide the Chrome extension (`extensions/chrome`): zip attached to each release now; Chrome Web Store later.
- [ ] Branch `main` (the local branch is `master`; rename before the first push).

Workflows (`.github/workflows/`, runner `windows-latest`):
- `ci.yml` on push and PR: `cargo fmt --check`, `cargo clippy -D warnings`, `cargo test` (tests that need a TeX install are skipped or marked `ignore` on CI), and in `ui/`: `tsc --noEmit`, `eslint`, `npm run test:visual`.
- `release.yml` on tag `v*`: build `moss-mcp`, run `tauri-apps/tauri-action`, upload the NSIS installer, signatures and `latest.json` to a **draft** release; the owner reads the notes and publishes.

First release: `v0.1.0-beta.1` marked "pre-release", since sync has not been run against a live Overleaf account yet. Release checklist: tests green, live Overleaf check done (or called out as untested in the notes), clean-machine install in Windows Sandbox, installer opens and compiles the tour Project, update from the previous beta works, notes written.

## Order
1. Logo checks + icons (section 1).
2. `moss-core::setup` and its tests, then the setup dialog and tour Template (section 2).
3. Installer, updater, versions (section 3).
4. Repo files, CI, first draft release (section 4).
