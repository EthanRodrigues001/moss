# Updates are signed and read from a fixed GitHub release

Moss installs updates itself with the Tauri updater. Two choices here are hard to undo.

- **The update key.** Updates are signed with a key made by `tauri signer generate`; its public half is in `tauri.conf.json` and every installed Moss trusts only that key. Lose the private key (kept at `~/.tauri/moss-updater.key`, and as the GitHub secret `TAURI_SIGNING_PRIVATE_KEY`) and no installed Moss can be updated again, only reinstalled. It has no password for now.
- **The feed.** The endpoint is `https://github.com/EthanRodrigues001/moss/releases/download/updater/latest.json`: a release with the fixed tag `updater` whose `latest.json` the release workflow overwrites each time. GitHub's own `releases/latest/download/` address skips pre-releases, and the first versions are betas, so it would never offer them. Renaming or moving the repository breaks the feed of every installed Moss, so the repository name is part of this decision.
- Updates are offered, never forced: the sidebar arrow and Settings > Updates show the version and notes, and the user chooses "Update and restart". The Windows installer runs in passive mode (progress bar, no questions).
- Windows code signing is separate and not done yet (unsigned betas show SmartScreen's "unknown publisher" warning); the update signature does not replace it.
