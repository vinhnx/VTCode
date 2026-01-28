extract vtcode-\* module as separate crates like tui-shimmer.

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

===

idea '/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-01-28 at 2.43.32 PM.png'

---

'/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-01-28 at 2.43.39 PM.png'

---

'/Users/vinhnguyenxuan/Documents/vtcode-resources/idea/Screenshot 2026-01-28 at 11.19.28 AM.png'

--

review https://www.openresponses.org/specification carefully and update if needed
on feature/openresponse-api-spec branch

check vtcode-core/src/open_responses/integration.rs for full integration
