# `moss-mcp` is a separate program built on the same Rust core as the app

Agents connect through a standalone `moss-mcp` stdio program rather than an MCP server embedded in the desktop app. It works on the Project folders directly, so agents keep working when the GUI is closed. The GUI and `moss-mcp` are therefore two processes sharing one Rust core library (`moss-core`).

The two processes share state only through files, not a database:

- **Approvals** are small JSON files in `<workspace>/.approvals/`. `moss-mcp` writes a pending request and polls it; the app lists pending ones and records the click. No daemon, no locking beyond atomic file writes.
- **History snapshots** live in each Project at `.moss/history/` (an index plus content-addressed file copies). This replaced the earlier plan of a git repository at `.moss/history.git` and of SQLite: a LaTeX Project is small, so whole-tree snapshots are cheap, and neither git nor a database has to be installed or bundled.

The cost is that the two processes can touch the same file at once. The app polls the open file and the file list every two seconds and reloads changes unless the user has unsaved edits.
