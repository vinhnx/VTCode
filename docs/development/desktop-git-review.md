# Git Working-Tree Review

Part of the [VT Code Desktop Workspace](desktop-workspace.md).

In the Changes inspector, choose **Review working tree**. Leave **Folder** empty
for the selected workspace or enter a relative directory, then choose **Load changes**.
No Git capture starts merely by opening the dialog. Folder resolution reuses the
shared workspace path resolver; missing folders, files, and paths escaping the
workspace (including symlink escapes) are rejected.

The snapshot combines tracked staged and unstaged changes against HEAD within
that directory. Untracked files are excluded by default; enable **Include untracked
text files**, then Load changes or Refresh to capture them. Git ignore rules still
apply. The snapshot remains separate from historical
agent-attributed session patches. It never stages, discards, applies, or commits
changes. The shared core Git reader disables external diff/textconv, clean/process
filters, and filesystem-monitor hooks, disables optional index refresh writes,
redacts secrets, and enforces a two-second
timeout. Explanation snapshots retain their existing 8 KiB limit; desktop review
uses a separate 200 KiB bound.

Untracked previews use the shared no-follow file reader and diff formatter, with
at most 80 files and 64 KiB per file. Symlinks, hard links, non-regular files,
binary/non-UTF-8 content, unsupported filenames, and files exceeding the remaining
preview budget are omitted and counted in the capture note. Any omission marks
the preview incomplete. An oversized or failed Git listing makes the capture
unavailable. Empty new files retain their added-file header. Untracked review
never stages files or changes the index. Explanation snapshots stay tracked-only.

Before the first commit, a valid unborn branch is compared with Git's empty tree
in the repository's native object format (SHA-1 or SHA-256). The capture note
identifies that baseline. The reader computes the empty-tree ID without writing
objects, creating an index, or making a commit. A corrupt branch reference,
unresolvable detached HEAD, or invalid repository remains unavailable rather than
being treated as an empty history. Existing committed and detached branches use
their resolved HEAD tree.

GPUI Kit renders complete supported patches in unified or split mode with soft
wrapping. **Previous file** and **Next file** jump to file headers and wrap at the
ends; the navigation row shows the selected path and its position. Refresh keeps
that path when it remains in the patch, even if files reorder, and otherwise
selects the first remaining file. Navigation is disabled for a single-file patch
and unavailable for raw or truncated previews.
Choose the current path to search all captured files without closing the review;
Escape first clears a non-empty query, then closes the picker and returns to the
same review snapshot.
**Copy preview** copies the captured redacted text. A truncated or
unsupported patch is shown as raw text with an explicit notice rather than as a
complete structured diff. Clean tracked state is distinct from invalid or
unavailable Git state. Use Refresh after edits; captures are snapshots, not a filesystem watcher.

The viewer reuses the session review dialog's focus trap and Escape restoration.
Refresh retains the display mode. Request and workspace identities reject late
captures after refresh, dismissal, or workspace changes. Service failures unblock
retry. Captured Git changes never enter the agent transcript or session history.
