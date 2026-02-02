[DON'T DELETE UNTIL FEEL COMPLETE] review duplicated and redundent logic from whole code base and remove and cleanup

continue with your recommendation, proceed with outcome. don't stop. review overall progress and changes again carefully, can you do better? go on don't ask me

--

the bottom view shimmer and progress loading state is not cleared when error occurs.

--

sometimes the escape sequence is leak when cancel the program with control+c

     Running `target/debug/vtcode --show-file-diffs --debug`

^C^C^C^C^C^C

--

15:25:26 ❯ ./scripts/run-debug.sh
VTCODE - Debug Mode (Fast Build)
==================================
Building vtcode in debug mode...
Compiling vtcode v0.74.10 (/Users/vinhnguyenxuan/Developer/learn-by-doing/vtcode)
warning: method `set_defer_rendering` is never used
--> src/agent/runloop/unified/ui_interaction.rs:502:8
|
398 | impl StreamingReasoningState {
| ---------------------------- method in this implementation
...
502 | fn set_defer_rendering(&mut self, defer: bool) {
| ^^^^^^^^^^^^^^^^^^^
|
= note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: `vtcode` (bin "vtcode") generated 1 warning
Finished `dev` profile [unoptimized] target(s) in 8.25s

Debug build complete!

Starting vtcode chat with advanced features...

- Async file operations enabled for better performance
- Real-time file diffs enabled in chat
- Type your coding questions and requests
- Press Ctrl+C to exit
- The agent has access to file operations and coding tools

Tip: Use 'cargo rrf' for fast optimized runs (release-fast profile)

warning: method `set_defer_rendering` is never used
--> src/agent/runloop/unified/ui_interaction.rs:502:8
|
398 | impl StreamingReasoningState {
| ---------------------------- method in this implementation
...
502 | fn set_defer_rendering(&mut self, defer: bool) {
| ^^^^^^^^^^^^^^^^^^^
|
= note: `#[warn(dead_code)]` (part of `#[warn(unused)]`) on by default

warning: `vtcode` (bin "vtcode") generated 1 warning
Finished `dev` profile [unoptimized] target(s) in 0.64s
Running `target/debug/vtcode --show-file-diffs --debug`
^C^C^C
