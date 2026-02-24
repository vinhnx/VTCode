- [x] fix changlog generator should generate from last changed version instead of since 0.71.0. currently it generates from 0.71.0 to current version, which is wrong if we have multiple versions after 0.71.0. We should generate from last changed version to current version. (Fixed in release.sh and release.yml)

--

https://github.com/google/bloaty

--

scan to improve panic and exception handling like force unwrap and expect.

---

---

more PTY truncate display in TUI, reference:

```

• Ran cargo test spool_chunk_read_path_detects_spooled_read_calls -- --nocapture
  └     Blocking waiting for file lock on package cache
        Blocking waiting for file lock on package cache
    … +346 lines
    finished in 0.00s



• Ran cargo check
  └    Compiling vtcode-config v0.82.1 (/Users/vinhnguyenxuan/Developer/
        Finished `dev` profile [unoptimized] target(s) in 57.13s

• Ran cargo test -p vtcode-config --no-run
  └     Blocking waiting for file lock on package cache
        Blocking waiting for file lock on package cache
    … +21 lines
      Executable tests/subagent_loading.rs (target/debug/deps/
    subagent_loading-19ca9abe89c74e0a)
```

---

review and revise ask_user_question