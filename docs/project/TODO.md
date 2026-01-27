improve UX and responsive when agent about to edit/write file, currently the TUI is not responsive and not immediate feedback to the user when agent is about to edit/write file.

--

the agent was able to use `rm` 


Run bash .. Timeout: 180 output

$ rm -rf /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target && sleep 2 && cargo clippy 2>&1 | head -n 200

rm: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target/debug/.fingerprint: Directory not empty
rm: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target/debug: Directory not empty
rm: /Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode/target: Directory not empty
The lock file is being held by a previous process. Let me kill any cargo processes and try again.

-> for destructive commands, the vtcode agent should use human in the loop with persmission prompt

--

--

style tools args in different ansi

'/Users/vinhnguyenxuan/Desktop/Screenshot 2026-01-27 at 5.35.30 PM.png'

example


`Search text Grep if !buffer\.is_empty\(\) \{ in vtcode-core/src/a2a/client.rs`

-> 

style `Grep if !buffer\.is_empty\(\) \{`, in different color

--

---

--

make sure the agent properly wait for PTY tools output before processing turn, to avoid repeatly wait and retrigger for locked commands (eg: cargo check, cargo test) 