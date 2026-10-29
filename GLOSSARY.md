# Moss

A local-first desktop LaTeX editor. Projects live as ordinary folders on the user's disk, compile with the user's own TeX installation, stay in sync with Overleaf, and can be driven by an AI agent.

## Language

### Documents

**Project**:
One folder on disk holding a complete LaTeX document and all its sub-files. Maps one-to-one to one Overleaf project.
_Avoid_: Document (as a separate concept), repo, workspace item

**Archive**:
A shelf in the sidebar for Projects the user is not working on. Archiving only moves a Project there; its files, History and Overleaf link stay as they are, and it can be moved back at any time. Different from deleting, which removes the Project's folder from this PC.
_Avoid_: Hide, trash

**Origin**:
Where a Project came from and whether it is linked to Overleaf, shown as a **Local** or **Overleaf** tag. A Project is Overleaf only while it has an Overleaf link; otherwise it is Local.
_Avoid_: Source, type, kind

**Import**:
Creating a new Project from files already on this PC, either a zip or a folder. The result is tagged Local.
_Avoid_: Upload (reserved for sending files to Overleaf), open

**Root file**:
The `.tex` file in a Project that carries `\documentclass` and is compiled to produce the PDF.
_Avoid_: Main file, entry point

**Workspace**:
The single folder that contains every Project on this machine.
_Avoid_: Library, vault

### Building

**Compile**:
Turning a Project's Root file into a PDF with the user's TeX installation, continuing past errors so a PDF is still produced.
_Avoid_: Build, render

**Build cache**:
Disposable per-Project compile output (auxiliary files and the PDF) kept only so later compiles are fast. Never synced or exported as source.
_Avoid_: Output, artifacts

**Page image**:
A compiled page drawn as a picture so a reader who cannot open a PDF, such as an Agent, can see the layout. One page, a range, or several pages laid out together in a single image (a **contact sheet**). Made from the last Compile and flagged when the sources have changed since.
_Avoid_: Screenshot, render (a Compile makes the PDF; a Page image only looks at it)

**Template**:
A starter document for a new Project (article, IEEE paper, report, thesis, slides, letter, CV, XeLaTeX, and the Moss tour) that compiles as it is.
_Avoid_: Preset, boilerplate

**Setup**:
The first-run (and Settings) step that checks this PC for what Compile needs (a TeX distribution, Perl, SyncTeX, Page image tools) and installs what is missing for the current user only. It never installs over another TeX distribution.
_Avoid_: Installer (that is the program that installs Moss itself), wizard

**Moss tour**:
The Template that is created at the end of Setup: a short document with maths, a figure, a table and a citation, with comments pointing at what Moss can do.
_Avoid_: Demo project, sample

**Update**:
A newer published version of Moss itself. Moss checks for one, shows it in the sidebar and in Settings, and installs it only when the user asks.
_Avoid_: Upgrade, patch

### Overleaf sync

**Sync**:
One user action that Pulls from Overleaf, merges, shows what will change, and Pushes the result. Always a Review first, then Apply.
_Avoid_: Update, refresh

**Review**:
The screen shown before any Pull, Push or Sync changes a file: every file that will be written, uploaded, merged or deleted, with its diff. Nothing changes until the user applies it.
_Avoid_: Preview, confirmation

**Overleaf session**:
The signed-in state Moss borrows from the user's Overleaf login, kept in Windows Credential Manager. Moss never holds the Overleaf password.
_Avoid_: Token, API key, account link

**Moss Connector**:
The small Chrome extension that hands the user's existing Overleaf session to Moss so no second sign-in is needed.
_Avoid_: Plugin, bridge

**Pull**:
Fetching the current state of a Project from Overleaf as a temporary zip, applying it locally, then deleting the zip.
_Avoid_: Download, import

**Push**:
Sending local changes (uploads, renames, deletions) to Overleaf after the user has seen them.
_Avoid_: Upload, publish

**Base snapshot**:
The copy of a Project as it was at its last successful Sync; the common ancestor for three-way merge.
_Avoid_: Last version, backup

**Conflict**:
A file changed on both sides since the Base snapshot in a way that cannot be merged automatically, so Sync stops and asks the user.
_Avoid_: Collision, merge error

**Mine / Theirs**:
In a Conflict, Mine is the copy on this PC and Theirs is the copy on Overleaf.
_Avoid_: Local/remote, ours/theirs, current/incoming

**Resolution**:
The user's answer to one Conflict: keep Mine, keep Theirs, keep both, or write the text by hand. Applied together with the rest of the Sync.
_Avoid_: Fix, merge choice

**Practice project**:
A folder on this PC that stands in for an Overleaf project, so Sync can be tried or tested without an account or any risk.
_Avoid_: Mock, demo, fake remote

### Agents

**Agent**:
An external AI client that edits Projects through Moss's MCP tools.
_Avoid_: Bot, assistant

**Tool**:
One action an Agent can ask Moss to do over MCP, such as `read_file` or `overleaf_pull`. Each has a title, a group (Projects, Reading, Writing, Build, Preview, History, Overleaf) and labels for what it can do: read-only, can overwrite or delete, asks for an Approval, uses Overleaf. The user can switch any Tool off in Settings, and Agents then do not see it.
_Avoid_: Command, function, capability

**History snapshot**:
A restore point of every file in a Project, taken automatically around an Agent write, a file-tree change, a Sync apply or a restore, or by the user with Save version. Every change, including a restore itself, can be undone and redone.
_Avoid_: Checkpoint, autosave

**Version**:
What the History tab shows for one History snapshot: a short id, a message, who made it (You, Agent, Overleaf), when, and how many files and lines it changed.
_Avoid_: Commit, revision

**Outside change**:
A change to a Project's files made by something other than the Agent tools (the user typing in the app, another program). The next Agent tool call notices it, keeps the files as a Version marked "Outside Moss", and tells the Agent.
_Avoid_: Conflict, external edit

**Project search**:
Finding (and optionally replacing) text in every text file of the open Project, from the search button or Ctrl+Shift+F. A replace takes one restore point first, so Undo brings everything back.
_Avoid_: Find in files, global search

**Working copy**:
The files on disk right now. The top row of the History tab, showing what changed since the latest Version.
_Avoid_: Draft, unsaved changes

**Approval**:
A click in the app that an Agent must wait for before a risky action (Push, delete, enabling shell-escape) runs, unless the Project is trusted. An unanswered Approval is denied after five minutes.
_Avoid_: Confirmation, permission prompt

**Trusted project**:
A Project where the user has chosen to skip Approvals for Agent actions.
_Avoid_: Auto-approve, unlocked

### Sync hygiene

**Ignore list**:
The built-in set of generated or private paths (such as `*.aux`, `*.log`, `.moss/`) that never take part in Sync or zip export.
_Avoid_: Exclusions, gitignore

**Needs attention**:
The list of files Sync skipped because they cannot be applied safely, such as names Windows cannot hold or two names differing only by case.
_Avoid_: Errors, failed files
