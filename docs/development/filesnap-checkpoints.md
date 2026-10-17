# Checkpoints and combined rewind

## Rewind in the terminal

Checkpoints are saved automatically before new prompts. In the chat input, type `/rewind`, select a prompt with the
arrow keys, press Enter, and choose `Rewind & Run` to restore both files and conversation. Both workspace files and the
conversation return to before that prompt. `/redo` restores both to before the last rewind; repeated redo walks
successive rewinds. A new prompt clears redo. Esc cancels the selector. The action picker also offers conversation-only
and code-only restoration.

Wait for the agent to finish before restoring. If a restore is interrupted, run `/rewind` again before continuing.
`/rewind` auto-recovers a pending restore: it resumes the interrupted restore and fails closed when nothing is pending,
never touching the redo stack. Recovery is journaled before file writes; conversation changes follow successful file
restoration. New combined history starts with prompts captured by this build, not legacy file-only checkpoints.

Capture is bounded. Ignored, remote, or uncaptured files are not protected. Known local write/edit paths are declared
before mutation, including absent files; binary files are restored as bytes. Capture failures never block the tool call:
unresolvable paths are skipped with a warning and the edit still runs, so rewind may not restore that edit. On Unix,
literal output redirects in simple POSIX shell commands (including `printf > file`, `cat > file` with a here-document,
and `>> file`) also declare their targets before execution, relative to the tool's working directory. Absolute targets
are canonicalized (macOS `/var` vs `/private/var`, symlinked workspaces, nearest existing ancestor for not-yet-created
files) and anything still outside the workspace is skipped. Repeated writes preserve the first pre-image in the prompt.
Dynamic destinations, scripts that change directory, nested shell scripts, interpreter file writes and other opaque
shell/MCP writes still need pre-existing capture coverage. These commands do not call a model.

Checkpoints made before shell redirection tracking was added may lack absence records for shell-created files. They
cannot safely infer which files to delete; use a new prompt checkpoint to validate this behavior.

The interactive runtime captures the conversation prefix before the user prompt and records file pre-images after tool
argument normalization. If checkpointing itself fails, the prompt is not sent: the message is removed from history and
restored into the input field so nothing is lost. Native list modals show only the current branch. Consumed or discarded
redo records are retired and their storage reclaimed instead of leaking. A workspace lease excludes concurrent
turns/restores. Ignore rules are pinned across each combined restoration.

Legacy CLI revert APIs retain their selected-file checkpoint semantics. New interactive checkpoints scan filesnap’s
three partitions. Existing V0/V1/V2 inline snapshots and V3 engine references remain readable through the legacy API.
Metadata and content use the configured checkpoint directory (default `.vtcode/checkpoints`); restoration refuses
symlink traversal, workspace escape, and overwriting checkpoint storage.
