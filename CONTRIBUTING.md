# Contributing

Issues and pull requests are welcome.

- Read `GLOSSARY.md` and use its terms in code, UI copy and docs.
- Do not undo a decision in `docs/adr/` without a superseding ADR.
- Run `cargo test -p moss-core`, and `npx tsc --noEmit` and `npm run lint` in `ui/`, before opening a pull request.
- Keep one version in all four files: `node scripts/bump-version.mjs <version>`.
