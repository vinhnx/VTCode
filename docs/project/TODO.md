improve UX and responsive when agent about to edit/write file, currently the TUI is not responsive and not immediate feedback to the user when agent is about to edit/write file.
-> instead of showing loading in spinner, show immediate ui/ux in the tui transcript log
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

Nobody should be using up arrows to get previous commands in a terminal or shell. You have to move your hand and its linear complexity in keystrokes. Use ctrl+p (for low n) or ctrl+r. Use a real shell or history manager (fish, fzf, atuin) for ctrl+r.

-> implement pseudo control+r -> show a list of all previous commands with fuzzy search (check @files picker impl)

