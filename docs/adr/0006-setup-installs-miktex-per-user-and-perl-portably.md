# First-run setup installs MiKTeX per-user and Perl portably, hash-pinned

ADR 0003 says Moss offers a one-click MiKTeX plus Perl install when no TeX is found. This records how.

- **Per-user, no admin prompt.** The MiKTeX basic installer runs with `--unattended --private`. Perl is Strawberry Perl's portable zip, unpacked to `%LOCALAPPDATA%\Moss\perl`. Neither changes `PATH`; detection (`tex::detect`) looks in those places itself.
- **Pinned downloads.** URL and SHA-256 of each installer are constants in `moss_core::setup`. Downloads are HTTPS only (loopback HTTP in tests), resumable, and a mismatch deletes the file and installs nothing. The cost is a source change whenever MiKTeX or Perl publishes a new version; we accept it over running unverified code.
- **TeX Live is never installed over.** If another distribution is found, Moss only reports what is missing.
- **MiKTeX is told to install packages itself** (`initexmf --set-config-value=[MPM]AutoInstall=1`), so a background compile never blocks on a prompt.
- **Cost.** About 450 MB of downloads: MiKTeX basic (149 MB) and Strawberry Perl portable (305 MB, it carries a C toolchain Moss does not need). Strawberry publishes no smaller Perl; revisit if a lean Windows Perl with a stable URL appears.
