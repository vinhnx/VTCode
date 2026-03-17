# core agent loop exploration

## Plan of Work

- [x] Map runloop modules
  outcome: Identified runloop/unified modules for loop logic (session loop, turn loop, contexts) by listing directory contents.
- [x] Trace session loop
  outcome: Reviewed `session_loop_impl.rs` and runner to trace session lifecycle: initialization, interaction loop, turn execution, idle handling, finalization.
- [x] Detail turn loop
  outcome: Examined `turn/turn_loop.rs`: phases, guard hooks, tool dispatch, recovery, notifications.
- [x] Summarize supporting components
  outcome: Captured key roles for RunLoopContext, HarnessTurnState, and tool pipeline modules supporting loop behavior.
