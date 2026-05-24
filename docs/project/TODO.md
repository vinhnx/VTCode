as of 2026-04-26 there are no formal benchmarks, so evaluate it on your own codebases.

The plan is to have a benchmark suite that runs on a regular basis, but it is not yet implemented.

---

warning: public documentation for `harness_exec_session_completed` links to private item `crate::tools::exec_session::ExecSessionManager::is_session_completed`
--> vtcode-core/src/tools/registry/harness_facade.rs:84:11
|
84 | ... [`crate::tools::exec_session::ExecSessionManager::is_session_completed`].
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this item is private
|
= note: this link will resolve properly if you pass `--document-private-items`
= note: `#[warn(rustdoc::private_intra_doc_links)]` on by default

warning: public documentation for `terminate_harness_exec_session` links to private item `crate::tools::exec_session::ExecSessionManager::terminate_session`
--> vtcode-core/src/tools/registry/harness_facade.rs:94:11
|
94 | ... [`crate::tools::exec_session::ExecSessionManager::terminate_session`].
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this item is private
|
= note: this link will resolve properly if you pass `--document-private-items`

warning: public documentation for `terminate_all_exec_sessions_async` links to private item `crate::tools::exec_session::ExecSessionManager::terminate_all_sessions_async`
--> vtcode-core/src/tools/registry/pty_facade.rs:52:11
|
52 | ... [`crate::tools::exec_session::ExecSessionManager::terminate_all_sessions_async`].
| ^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^^ this item is private
|
= note: this link will resolve properly if you pass `--document-private-items`

warning: `vtcode-core` (lib doc) generated 3 warnings
