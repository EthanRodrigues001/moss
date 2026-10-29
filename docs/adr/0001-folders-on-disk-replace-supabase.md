# Projects are plain folders on disk; Supabase is removed

The web version of Moss stored projects in Supabase Postgres and Storage. Moss is now a single-user, local-first desktop app that syncs with Overleaf, so each Project is an ordinary folder laid out exactly like its Overleaf file tree. This makes compiling, diffing, zipping and three-way merge trivial, and lets TeX see `.cls`, `.sty` and fonts directly. We gave up multi-device access and cloud login; Overleaf itself is the cloud copy.

A single workspace-wide SQLite file holds only data that can be rebuilt (search index, caches) plus the audit log and approval queue. It is not a source of truth for any Project content.
