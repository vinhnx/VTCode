[DON'T DELETE UNTIL FEEL COMPLETE] review duplicated and redundent logic from whole code base and remove and cleanup AND DRY.

continue with your recommendation, proceed with outcome. don't stop. review overall progress and changes again carefully, can you do better? go on don't ask me

--

Background terminal Run long-running terminal commands in the background.

suggest ui

```
Background terminals

  • cargo clippy -p vtcode-core --no-deps
    ↳     Checking vtcode-exec-events v0.75.2 (/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcod [...]
          Checking vtcode-file-search v0.75.2 (/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcod [...]
          Checking vtcode-bash-runner v0.75.2 (/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcod [...]

• Waiting for background terminal · cargo clippy -p vtcode-core --no-deps (36m 38s • esc to interrupt)
  1 background terminal running · /ps to view
```

---

Shell snapshot Snapshot your shell environment to avoid re-running
login scripts for every command.

---

improve git changelog generator to group by commit types and show more structured changelog output. https://github.com/openai/codex/releases/tag/rust-v0.95.0

---

---

fix and implement insta snapshot test
cargo insta review.

--

fix sccache

--

VTcode could really slow/laggy now
Working on addressing this. Several things to fix but don’t have a “one thing” cause yet:

- Reading same large .jsonl files multiple times
- Too many process spawns (about half of this is fixed)
- Blocking sync writes can happen while typing
- macOS’ Gatekeeper causes slowdown
