# Tool reentrancy guard

The private `tools/registry/reentrancy.rs` module owns recursion tracking and
frame cleanup. `execution_facade.rs` owns admission, error payloads, execution
history, and the guard's lifetime around execution.

Each Tokio task has its own stack. Calls outside a Tokio task use a thread-local
stack. A repeated tool name is rejected, including cycles through other tools.
The stack may contain at most 64 active frames.

The facade permits same-tool siblings only for prevalidated, parallel-safe
calls. This allowance retains the depth limit. It does not grant permission or
replace any safety checks.

Each guard stores a unique frame ID. Dropping a guard removes that frame even
when guards finish out of order. Task guards retain their originating task ID
for cleanup when dropped elsewhere; thread fallback cleanup uses the local
thread's stack. Empty task stacks are removed from the global map. Cancellation
and unwinding use the same cleanup. Map locks and thread-local borrows are held
only during entry or cleanup, never across an await.

Run the guard boundary tests and public registry recursion regressions with:

```sh
cargo nextest run --locked -p vtcode-core -E 'test(reentrancy)'
```

For execution admission and policy coverage, also run:

```sh
cargo nextest run --locked -p vtcode-core -E 'test(tools::registry::tests)'
```
