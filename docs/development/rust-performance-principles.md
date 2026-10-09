# Rust-Specific Performance Principles for VT Code

This document captures the nuance of what makes Rust fast (and where it isn't) in the context of the vtcode project. It
complements the general guidelines in `performance.md` by focusing on Rust-specific properties that affect the
optimizer, the standard library, and day-to-day coding decisions.

## Table of Contents

- [Core Insight: Rust Is Not Faster Than C/C++ — It Is _Safer While Being Equally Fast_](#core-insight-rust-is-not-faster-than-cc--it-is-safer-while-being-equally-fast)
- [Destructive Move Semantics](#destructive-move-semantics)
- [Aliasing Guarantees (`noalias`)](#aliasing-guarantees-noalias)
- [Immutable by Default & `const` Semantics](#immutable-by-default--const-semantics)
- [Bounds Checking & Iterator Elision](#bounds-checking--iterator-elision)
- [The `#[cold]` and `#[inline]` Strategy](#the-cold-and-inline-strategy)
- [ABI Stability & Standard Library Evolution](#abi-stability--standard-library-evolution)
- [LLVM's C/C++ Legacy: Why Rust's Extra Information Does Not Always Translate](#llvms-cc-legacy-why-rusts-extra-information-does-not-always-translate)
- [Safety Enables Aggressive Optimization](#safety-enables-aggressive-optimization)
- [When Rust Can Be Slower Than C/C++](#when-rust-can-be-slower-than-cc)
- [Breaking Inter-Iteration Dependencies (Value Speculation)](#breaking-inter-iteration-dependencies-value-speculation)
- [Branchless Programming: Removing Unpredictable Branches](#branchless-programming-removing-unpredictable-branches)
- [Checklist for VT Code Hot Paths](#checklist-for-vt-code-hot-paths)

---

## Core Insight: Rust Is Not Faster Than C/C++ — It Is _Safer While Being Equally Fast_

For a well-optimized program, Rust and C++ produce comparable machine code. The performance differences are marginal and
situational. The real advantage of Rust is that it makes it _easier_ to write fast, correct code without compromising
safety. In C++, defensive programming (extra copies, conservative synchronization) erodes performance when engineers are
not operating at peak expertise. Rust's type system eliminates the need for much of that defensive overhead.

Note that **C is not the "diamond standard" of performance** — that title arguably belongs to **Fortran**, whose
stronger aliasing guarantees (no pointer aliasing at all) have enabled decades of superior numerical optimization.
Rust's ownership model places it in a similar position to Fortran: the compiler _knows_ references are unique, whereas C
requires the explicit `restrict` keyword (rarely used in practice). Rust is structurally positioned to match or exceed
C's optimization ceiling, but realizing that potential depends on the backend's ability to consume the information —
which brings us to LLVM.

**VT Code implication**: When choosing between a safe and an `unsafe` implementation, prefer the safe one and measure
first. The borrow checker gives the optimizer information that C++ cannot express, so safe Rust can _already_ produce
better code than C++ in many cases.

---

## Destructive Move Semantics

Rust moves are _bitwise_: they copy the bytes and the source is no longer considered valid. In C++, a moved-from object
must remain destructible, so the move constructor leaves behind a valid (often empty) state and the destructor still
runs. This has two consequences:

1. **No post-move cleanup**: Rust's `Vec::pop`, `String::pop`, `std::mem::take`, and `Option::take` all generate
   simpler, more optimizable assembly than their C++ counterparts.

2. **Realloc works**: `Vec` can use `realloc` on growth because moves are bitwise. C++ `std::vector` cannot safely
   `realloc` non-trivial types.

### VT Code guidelines

- Use `std::mem::take(&mut value)` instead of `.clone()` followed by `.clear()` when you need to move a value out of a
  `&mut` reference.
- Use `Option::take()` for the same pattern with `Option<T>`.
- Prefer `Vec::pop()` over indexed removal when order doesn't matter.
- Use `Vec::drain(..)` instead of manual element-by-element moves for bulk extraction.

**Already applied**: `std::mem::take` is used in 24+ locations across vtcode-core (agent runtime, events, stream buffer,
pipeline, etc.). Continue this pattern.

---

## Aliasing Guarantees (`noalias`)

The single biggest theoretical advantage Rust has over C/C++ in the optimizer is pointer aliasing information:

- `&mut T` is guaranteed to be _unique_ — no other reference can alias it. This is equivalent to C's `restrict` keyword,
  applied implicitly to every mutable reference.
- `&T` is guaranteed to be _immutable_ — the value cannot mutate while the reference exists.

C++ `const T&` does _not_ carry this guarantee: `const_cast` can remove const-ness, and mutable aliases may exist. The
optimizer must assume the worst.

### History: Rust as an LLVM bug finder

Rust's aggressive emission of `noalias` has historically been a rollercoaster. The feature was initially enabled around
2014–2015 after Rust settled on `&mut` semantics, then deactivated due to LLVM bugs. It was re-enabled and quickly
deactivated again in 2018. Finally, with LLVM 12 (Rust 1.54+), `-Zmutable-noalias=yes` was enabled by default.

Before each deactivation, Rust's `noalias` emission **revealed multiple bugs in LLVM** — bugs that existed but were
never triggered because no C/C++ frontend emitted `noalias` as aggressively. In effect, Rust has been a stress-test for
LLVM's alias analysis, improving codegen for all LLVM frontends (including Clang). Fortran (via gfortran) similarly
exercises GCC's aliasing paths, which is why GCC's handling has historically been more robust — but LLVM's Flang
frontend is younger and hasn't yet had the same shake-down.

As of Rust 1.54+ / LLVM 12+, `&mut T` in vtcode gives LLVM _actionable_ alias information that C++ cannot express.

### VT Code guidelines

- Prefer `&mut T` over raw pointers to communicate non-aliasing intent.
- When writing hot loops over slices, use `&mut [T]` and `&[T]` rather than `*mut T`/ `*const T` — the optimizer gets
  alias info for free.
- Use `split_at_mut` for slice subdivisions instead of raw pointer arithmetic.
- Avoid `UnsafeCell` unless profiling proves it necessary — it suppresses alias analysis.

---

## Immutable by Default & `const` Semantics

In C++, `const` can be cast away with `const_cast`, so the optimizer cannot fully trust it. In Rust:

- `&T` is truly immutable (there is no safe `const_cast` equivalent)
- Values are immutable by default; `mut` is explicit

This means the Rust compiler (and LLVM) can cache loaded values across function calls without reloading. In C++, a
function receiving `const int&` must reload after every call because the callee might have cast away const.

### VT Code guidelines

- Use `&T` rather than `&mut T` wherever mutation is not needed — it communicates aliasing safety to the optimizer.
- Use `&str` rather than `&String` in function parameters.
- Use `&[T]` rather than `&Vec<T>` in function parameters.
- Make fields `pub` only when needed; prefer immutable public API surfaces.

---

## Bounds Checking & Iterator Elision

Rust performs bounds checking on array/slice indexing by default. In hot loops, this can inhibit vectorization and other
optimizations when the compiler cannot prove the bounds.

The real cost of bounds checks is rarely the arithmetic itself — it is the **cascading failure of pattern-matching in
the optimizer**. LLVM optimizations are largely pattern-based: if a bounds check creates IR that doesn't match a
vectorization or loop-hoisting pattern, the compiler may miss entire families of optimizations downstream. The check
itself may add zero measurable cycles, but the optimizations it blocks can cost double-digit percentages.

_However_:

- Iterator patterns (`for x in slice`, `.iter()`, `.iter_mut()`, `.chunks()`) elide bounds checks entirely because the
  iterator guarantees in-bounds access.
- The optimizer often eliminates bounds checks in `for i in 0..slice.len()` loops.
- `unsafe` is available for the rare cases where the compiler cannot prove safety.

### VT Code guidelines

- Prefer iterator combinators (`map`, `filter`, `fold`, `for_each`) over indexed loops in hot paths.
- Use `for x in &slice` / `for x in &mut slice` instead of `for i in 0..slice.len() { slice[i] ... }`.
- Use `.chunks()` and `.windows()` for sliding-window access to elide per-element bounds checks.
- Only use `unsafe { get_unchecked() }` when profiling proves bounds checks are a bottleneck.

**Measured in vtcode**: Indexed `for i in 0..N` loops are rare in core hot paths (found mostly in tests and memory_pool
setup). This is good.

---

## The `#[cold]` and `#[inline]` Strategy

The `#[cold]` attribute tells LLVM that a function is unlikely to be executed. This causes LLVM to:

- Move the cold code to a separate section (improving instruction cache locality for hot paths).
- Not inline the cold function (shrinking hot-path code size).

This is directly analogous to how C++ compilers move exception-handling code to cold sections (GCC
`-freorder-blocks-and-partition`).

### Where to use `#[cold]`

- Error reporting and formatting functions
- Warning/diagnostic paths
- Recovery and fallback logic
- Rarely-invoked initialization
- Any path that branches on "should not happen" conditions

### Where to use `#[inline]`

- Small functions (≤10 lines) in documented hot paths
- Functions whose call sites benefit from constant propagation
- Generic functions where monomorphization makes inlining cheap

### Where _not_ to use `#[inline]`

- Large functions — inlining them bloats code size and pollutes the instruction cache
- Functions only called from one place (LLVM will inline them anyway if profitable)
- Error-only paths (mark these `#[cold]` instead)

### VT Code current state

| Annotation         | Count | Assessment                                                                       |
| ------------------ | ----- | -------------------------------------------------------------------------------- |
| `#[inline]`        | ~150  | Good coverage on hot small functions                                             |
| `#[cold]`          | ~75   | Well-covered; most error-diagnostic paths are annotated.                         |
| `#[inline(never)]` | few   | Manual `Debug` impls on fan-out error types; see Derived trait impls below.      |

**Action**: When adding new error-only functions, annotate them `#[cold]` rather than `#[inline]`.

### Derived trait impls are `#[inline]`

`#[derive(Debug)]` (and `Clone`, `Default`, …) expands to an impl whose generated methods
carry `#[inline]`. This is implied by the [reference](https://doc.rust-lang.org/reference/attributes/derive.html)
but is not a guaranteed contract. For trivial types it is exactly what we want: the impl
inlines away and costs nothing.

It becomes a size hazard for **large or deeply nested types**, most commonly the error enums
VT Code formats on failure paths. A derived `Debug::fmt` for a wrapper error calls its child's
`Debug::fmt`, and because both are `#[inline]`, rustc can inline the whole tree into every
`format!("{:?}", err)` / `tracing::warn!(error = ?err)` call site. rustc does not appear to
bound the size or number of these inlinings, so the cost is paid **per call site**, not per
type — a nested hierarchy can dominate a binary's code size (`uv` reclaimed ~160 KB this way).

When a type's `Debug` is large or formatted on a fan-out path:

- Annotate its `Debug::fmt` with `#[inline(never)]` — either by hand-writing the `impl`
  (as [`LLMError`/`LLMErrorMetadata`](../../crates/common/vtcode-commons/src/llm.rs) already
  do) or with the `DebugNoInline` derive from `vtcode-macros`.
- Leave small, hot, leaf `Debug` impls derived. `#[inline(never)]` on a tiny struct whose
  `Debug` is called in a tight loop can make things slower.

VT Code's release profile (`opt-level = "z"`, fat LTO, `strip`, `panic = "abort"`) already
garbage-collects `Debug` impls that are never referenced, so only impls reached by live `{:?}`
sites contribute. Measure before converting — see the derived-trait recipe in
`docs/analysis/BLOATY_ANALYSIS.md`.

---

## ABI Stability & Standard Library Evolution

C++'s standard library is constrained by ABI stability: `std::unordered_map` is locked into a node-based design,
`std::regex` cannot switch to a faster implementation, and `std::string` cannot drop its small-string-optimization
layout without breaking linked binaries.

Rust has no stable ABI for the standard library. This means:

- `HashMap` in `std` was replaced by `hashbrown` (a Swiss-table implementation) — significantly faster than C++
  `std::unordered_map`.
- The standard library can adopt new data structures and algorithms without breaking existing binaries.

### VT Code implications

- vtcode already uses `hashbrown::HashMap` directly (~370 uses) and `rustc_hash::FxHashMap` for measured hotspots. This
  is correct.
- Unlike C++ projects, vtcode does not need third-party hash map replacements; `hashbrown` is already the best
  available.
- The `regex` crate (used via dependencies) is already faster than C++ `std::regex` due to its compiled-once,
  automata-based approach.

---

## LLVM's C/C++ Legacy: Why Rust's Extra Information Does Not Always Translate

Despite Rust's richer semantic information, LLVM — the primary backend for `rustc` — was designed and optimized for
C/C++ over two decades. This creates several bottlenecks:

### Niche information is dropped

Rust guarantees niches: `&T` is never null, `&u16` is always 2-byte aligned, `bool` is only 0 or 1, etc. Rust's internal
type system tracks these, but LLVM has no first-class concept of niches — C and C++ do not have them. When rustc lowers
to LLVM IR, most niche information is either discarded or represented in ways LLVM cannot exploit. Active work exists to
improve this, but LLVM's IR was not designed for it.

### No optimized calling convention for sum types

Rust uses `Option<T>` and `Result<T, E>` pervasively. These are tagged unions (discriminant + payload). C has tagged
unions too, but no ABI or calling convention optimizes their passing — e.g., passing the discriminant in a flag register
and splitting variants across registers vs. stack. Neither GCC nor LLVM support such conventions because C never needed
them. This means returning `Result<T, E>` from a function can involve unnecessary memory traffic that a hypothetical
optimal calling convention would avoid.

### Move-heavy codegen is less tuned

Rust's pervasive move semantics (bitwise copy + source invalidation) are uncommon in C/C++. When constructing a
`Box::new(value)`, Rust constructs the value on the stack then copies it to the heap. LLVM can elide this copy
(NRVO-style), but the pattern-matching isn't always successful. Equivalent C code (allocate on heap, initialize
in-place) generates simpler IR from the start.

### What this means for vtcode

These are backend limitations, not language limitations. As LLVM evolves (or if Rust gains an alternative backend like
GCC or Cranelift), these gaps will narrow. For vtcode's workload (I/O-bound LLM orchestration, not tight numeric loops),
these issues are unlikely to be material — but they explain why Rust's "free performance from information" has not
materialized at scale.

## Safety Enables Aggressive Optimization

The most practically significant performance difference between Rust and C++ in a real-world project is not compiler
optimization — it is the _social and architectural_ effect of safety.

In C++, developers introduce:

- **Defensive copies**: to avoid lifetime bugs.
- **Conservative locking**: to avoid data races.
- **Shallow abstractions**: to avoid the risk of unsafe pointer manipulation.
- **Coarse-grained ownership**: because fine-grained ownership is too error-prone.

Each of these "defense in depth" decisions has a performance cost. Rust eliminates the need for them:

- `&T` is guaranteed safe — no defensive `clone()` needed.
- `&mut T` is guaranteed unique — no locks needed for exclusive access in single-threaded code.
- The type system encodes ownership — no reference-counting overhead for clear ownership trees.
- `Send + Sync` provides compile-time data-race freedom — no runtime checks.

### VT Code guidelines

- When you find yourself adding a `.clone()` to appease the borrow checker in a hot path, consider changing the data
  structure or ownership model instead. A reference (`&T`) or a move (`std::mem::take`) is usually cheaper.
- Do not reach for `Arc<RwLock<T>>` by default. A `&mut T` or a simple `Box<T>` with exclusive access is faster.
- Use `Rc<T>` for single-threaded shared ownership when the reference is immutable; avoid `Arc` unless cross-thread
  sharing is proven necessary.
- Through a lock guard (`MutexGuard`, `RwLockWriteGuard`, `parking_lot` guards) every field access calls
  `Deref`/`DerefMut` on the whole guard, so the borrow checker cannot see `guard.a` and `guard.b` as disjoint. Reborrow
  once (`let state = &mut *guard;`) instead of cloning, copying fields out early, or splitting the critical section.
  Example: `ToolRegistry::record_tool_latency`. Source: Tyler Mandry, "Beyond the &", RustConf 2026.

---

## When Rust Can Be Slower Than C/C++

Rust has a few areas where it may be slower:

| Area                               | Why                                                                                                                                                                | Mitigation                                                                                               |
| ---------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------ | -------------------------------------------------------------------------------------------------------- |
| **Floating-point math**            | No global `-ffast-math` equivalent in safe Rust. LLVM strict FP semantics prevent many optimizations.                                                              | Use `-C llvm-args=-enable-unsafe-fp-math` for measured numeric hot paths, or target-specific intrinsics. |
| **Result checking in tight loops** | `Result<T, E>` is always checked; exceptions in C++ can be truly zero-cost when the sad path is rare.                                                              | Use `.unwrap_unchecked()` in `unsafe` blocks where invariants guarantee success (profile first).         |
| **Bounds checking**                | Default indexing includes bounds checks.                                                                                                                           | Use iterators or `get_unchecked()` when proven necessary.                                                |
| **Move-heavy heap allocation**     | Rust constructs values on the stack then copies to heap (`Box::new(val)`); LLVM does not always elide the intermediate copy. C allocates and initializes in-place. | Use `Box::new_uninit()` + manual init for measured hot paths, or arena allocation patterns.              |
| **Panic infrastructure**           | Panic unwinding has overhead even if panic never occurs.                                                                                                           | Use `panic = "abort"` in release (already vtcode's default).                                             |
| **Compile time**                   | Not a runtime concern, but Rust's generics and monomorphization increase build times.                                                                              | Keep `codegen-units=1` for fat-LTO release/bench; high units for dev/CI where parallelism wins. Keep portable linker/CPU defaults; `target-cpu=native` is local-only via `scripts/perf/native-*.sh`. No nightly `-Z` flags on stable. |

For vtcode, none of these are material concerns given the workload characteristics (I/O-bound LLM calls, not tight
numeric loops).

---

## Integer Overflow Checking: Near-Zero Cost with Proper Optimization

A common intuition is that checked arithmetic (panicking on overflow) imposes significant runtime cost. Production
experience from a former Microsoft Midori team compiler engineer ([source](https://ed0u11h)) demonstrates this is not
the case: with proper compiler support, the overhead of overflow checking on **every** arithmetic operation was
"literally unmeasurable" for most workloads, and at most 1.2% tax in the worst case.

### Why checked arithmetic is cheap in a well-designed compiler

1. **Late lowering**: The compiler keeps "add with overflow" as a single opcode in its IR throughout all optimization
   passes. Only at the very end — during machine-code lowering — does it emit the `add` + `jo` (jump on overflow)
   sequence. This means no optimization is inhibited by the presence of overflow checks — they don't break basic blocks,
   don't block vectorization, and don't impede code motion.

2. **Range analysis eliminates unnecessary checks**: If the compiler can statically prove an operation cannot overflow
   (e.g., `(i & 0xFF) + 0x1000` where both operands are bounded), it simply omits the check. This creates a **virtuous
   cycle**: checked arithmetic constrains the range of values, which lets the compiler eliminate checks on downstream
   operations, which in turn enables better optimization of subsequent code.

3. **Overflow coalescing**: Expression reassociation reduces the number of checks. `n + 4 + 4` is rewritten to `n + 8`,
   requiring only one overflow check instead of two. These patterns arise naturally in generated code (e.g., RPC
   serialization stubs) and compilers that treat checked arithmetic as a first-class optimization target handle them
   automatically.

4. **Inlining is the amplifier**: Inlining gives range analysis broader visibility into callers' invariants. A function
   like `checked_add(high, 1)` inlined into a context where `high < MAX` eliminates the check entirely. The tighter the
   language guarantees, the more the optimizer can eliminate.

### Checked arithmetic enables _better_ optimization

A subtle but important point: checked arithmetic makes the optimizer's job **easier**, not harder. When an operation
would overflow, all subsequent code is dead (execution jumps to the panic handler). The compiler does not need to
consider those states. Compare with C/C++ where signed overflow is undefined behavior — the compiler assumes it never
happens, but the programmer cannot assume the same thing. In Rust, overflow is defined behavior (panic in debug, wrap in
release), which means the compiler has _more_ constraints it can exploit, not fewer.

### What this means for vtcode

vtcode already follows best practices:

| Practice                       | vtcode status                                                                                                  |
| ------------------------------ | -------------------------------------------------------------------------------------------------------------- |
| Profile-based overflow control | `overflow-checks = true` in test, `false` in CI/release — correct split                                        |
| Semantic overflow methods      | `checked_*` for fallible paths, `saturating_*` for clamping, `wrapping_*` for hashing — all used appropriately |
| Hash code uses `wrapping_mul`  | FNV-1a, MurmurHash3 — wrapping is the intended semantics, no checks needed                                     |
| No `unchecked_*` intrinsics    | Appropriate — vtcode is I/O-bound, not tight numeric loops                                                     |

Guidelines for ongoing work:

- **Do not avoid `checked_*` in hot paths out of performance fear**. The optimizer handles it well. Use it where
  overflow indicates a real bug.
- **Prefer `wrapping_*` for hash computations** (already done) — this communicates intent and avoids test-mode panics.
- **Use `saturating_*` for UI/cursor/size math** (already done in TUI) — clamping is the correct semantics for layout.
- **Only reach for `unsafe { unchecked_add() }` when profiling proves a bottleneck** — this has not been necessary in
  vtcode to date.
- **Leverage the test profile**: since `overflow-checks = true` in `[profile.test]`, any arithmetic overflow in tests
  panics immediately, catching bugs that would silently wrap in release.

The Rust compiler's overflow checking is not yet at the level of the Midori compiler described above (rustc's MIR does
not keep overflow-checked ops as single nodes through all optimization passes — LLVM sees the branch). However, the
direction of travel is the same, and for vtcode's workload, the cost is already negligible.

---

## Breaking Inter-Iteration Dependencies (Value Speculation)

Modern CPUs run instructions out-of-order and rely on the _branch predictor_ to speculate past conditional jumps. A
tight loop that **threads a value through iterations** (e.g. `j = table[i][j]`) serializes: iteration _n+1_ cannot start
until iteration _n_ finishes, so throughput is bounded by the **latency** of the dependent load (a cache hit or an
indirect read), not by compute.

The fix (from "value speculation" / the _"useless if"_ trick): **speculate that the carried value is unchanged and only
reload it on a rare, predictable branch.** The predictor then hides the dependency, turning a latency-bound loop into a
throughput-bound one.

```rust,ignore
// latency-bound: every iteration waits on the reload
for i in ..n { j = table[i][j]; }

// throughput-bound: predictor assumes the branch is NOT taken
for i in ..n {
    if j != table[i][j] { j = table[i][j]; } // rarely taken
}
```

### Transferring this to VT Code

- **Literal pointer-speculation does _not_ apply here.** VT Code has no linked-list / `next`-pointer structures; hot
  paths use contiguous `Vec` / `VecDeque` / `HashMap`, which the hardware stride prefetcher already covers.
- **Rust has no stable `likely`/`unlikely`**, so the exact `[[unlikely]]` / `volatile` trick from C/++ does not port.
  The portable equivalent is to **carry the predicted state in a local and only touch the container on change.**
- Applied changes:
  - `vtcode-commons` `ansi::strip_ansi` — uses `memchr::memchr` (SIMD) to locate ESC bytes instead of a scalar byte scan
    (a latency-bound delimiter search made data-parallel).
  - `vtcode-indexer` `query` — scores `files`/`directories` in **parallel rayon chunks**, reusing one matcher + haystack
    buffer per worker thread (`map_init`); this realizes the same "more instructions in parallel" goal at thread
    granularity for large indexes.
  - `vtcode-ui` `coalesce_adjacent_spans` — hoists the carried `Style` into a local, appending to the tail only on a
    style change (the common path no longer re-reads `merged.last().style` per element).

### Caveat

The source articles warn L1-cache _hits_ are almost never the real bottleneck. Only apply this family when a profile
shows a dependency-bound tight loop. Measure before/after (`cargo bench`); revert if the change does not improve.

## Branchless Programming: Removing Unpredictable Branches

A companion to the Value Speculation section above. Both target **branch misprediction**, but they fix different shapes
of it — and the distinction matters because the fixes are not interchangeable:

- **Value speculation** (above): a _carried_ value serializes the loop (`j = table[i][j]`). The fix speculates the
  carried value is _unchanged_ and only reloads on a _rare, predictable_ branch. The branch stays; we make it easy to
  guess.
- **Branchless** (this section): a branch on _unpredictable_ data forces the predictor into a coin flip (≈50%
  misprediction). The fix removes the branch entirely, turning a control dependency into a data dependency.

### The cost of a misprediction

A modern CPU runs a deep pipeline and speculatively executes past branches it hasn't resolved yet, guessing the outcome
with a branch predictor. A correct guess is nearly free; a wrong guess flushes the pipeline and restarts — **~15–20
cycles on a typical x86 core**, against ~1 cycle for the comparison itself. The penalty is invisible when the branch is
predictable (almost always taken, or almost always not), and brutal when the outcome is a coin flip.

The diagnostic signature: a filter whose runtime _peaks near 50% selectivity_ on shuffled data, and is **4–5× faster on
the same data sorted** (so the branch goes "skip…skip…keep…keep" in two long runs the predictor learns). If sorting the
input changes the runtime dramatically, misprediction is the villain — not allocation, not the work itself.
(Preallocation is _not_ the fix: it typically saves ~2%; the misprediction penalty is the gap.)

### The branchless transform

Replace "decide _whether_ to write" with "always write, conditionally advance the cursor":

```rust,ignore
// idiomatic — branch on unpredictable data, ~50% mispredicted
let out: Vec<f64> = input.iter().copied().filter(|&x| x > threshold).collect();

// branchless — comparison becomes a number, not a fork (~4× faster at 50%)
let mut out = vec![0.0; input.len()];
let mut n = 0;
for &x in input {
    out[n] = x;                    // unconditional write
    n += (x > threshold) as usize; // seta: 0 or 1, no branch
}
out.truncate(n);
```

The comparison `(x > threshold) as usize` lowers to a `seta`/`setg` instruction that produces 0 or 1 with no fork in the
road; a rejected value is simply overwritten on the next kept iteration. The bounds check on `out[n]` and the loop
condition are _still_ branches, but they go the same way every iteration, so the predictor handles them for free. Only
the _unpredictable_ branch had to go — turning a control dependency into a data dependency.

### It is a trade, not magic — reserve it for measured hot paths

Branchless is **not** universally faster. The source article's benchmark (1M random `f64`, Intel i7-10875H):

| kept | idiomatic | branchless |
| ---- | --------- | ---------- |
| 1%   | 0.59 ms   | 1.09 ms    |
| 50%  | 3.94 ms   | 1.03 ms    |
| 99%  | 1.49 ms   | 1.11 ms    |

At 1% and 99% (well-predicted branches) the idiomatic version _wins_, because the branch is nearly free while branchless
always pays for N unconditional writes. Branchless trades the best case for the worst case, and the worst case becomes
flat (data-independent). **Only apply it when a profiler points at a hot loop _and_ the loop branches on data whose
outcome is genuinely unpredictable** (≈50% selectivity with no learnable pattern). Apply the sorted-vs-shuffled
diagnostic first; if sorting doesn't change the runtime, misprediction is not the problem and branchless won't help.

### When NOT to use it (VT Code-specific)

Most VT Code hot paths are **I/O-bound** (LLM network calls, disk reads, PTY output) — a 15–20 cycle misprediction is
noise next to a millisecond RTT. Branchless is for tight _in-memory_ loops, and the current hot paths largely aren't:

- **Delimiter byte-scans** (`byte == b'\n'`, `byte == b'\0'`, ESC) → use the `memchr` crate (SIMD), not branchless
  arithmetic. Delimiters are usually _rare_, so the branch is well-predicted (the idiomatic best case); branchless would
  make the common no-delimiter case do unnecessary work. `memchr` is already the repo pattern — `vtcode-commons`
  `ansi::strip_ansi` uses `memchr::memchr` to find ESC bytes, and `vtcode-memory` `event_log` reads lines via
  `BufReader::read_until(b'\n')` rather than a hand-rolled scan. The PTY scrollback ASCII newline scan and SSE/PTY line
  splitting fall here.
- **I/O-paced line loops** (SSE `data:` extraction, JSONL reconstruction) → the loop is paced by network/disk, not a
  tight CPU scan; per-line parsing dwarfs the branch. Branchless is noise.
- **Bounded result loops** (`grep_file::finalize_matches` match/context scan, `search_memory` fact filtering) → bounded
  by `max_results` / session count, and per-element work (JSON `Value::get` HashMap lookup, case-fold + substring
  search) is orders of magnitude more expensive than a branch. Branchless wouldn't move the needle.
- **Predictable predicates** (`if line.is_empty() { continue }`,
  `match serde_json::from_str { Ok => .., Err => continue }` on a valid log) → the branch goes one way almost always;
  this is the idiomatic best case.

### Decision audit (2026-08-06)

A sweep of the I/O-adjacent hot paths (`vtcode-indexer`, `vtcode-memory`, `vtcode-llm` streaming, `vtcode-core`
grep/PTY, `vtcode-exec-events`) found **no current loop that is both (a) a tight in-memory scan and (b) branching on
unpredictable data**. Every candidate was classified into one of the "when NOT to use it" buckets above. The technique
is recorded here so that when a _future_ profiler trace points at a genuine 50%-selectivity in-memory filter the fix is
obvious — and so the `memchr`-vs-branchless and value-speculation-vs-branchless distinctions are not re-derived. Full
candidate-by-candidate reasoning lives in `.vtcode/memory/branchless-2026-08-06.md`.

## Enum Footprint in Bulk Collections

Rust lays out an enum as discriminant + payload of the largest variant, with alignment padding. A "small" enum can
therefore be several times larger than any individual variant — a 15-variant enum whose every payload fits in 8 bytes
still occupies 16 bytes because the 8-bit discriminant forces 16-byte alignment (the motivation behind
[Replacing a Rust Enum with a 64-bit Word](https://pointersgonewild.com/2026-08-25-replacing-a-rust-enum-with-a-64-bit-word/),
where shrinking a 16-byte value union to one machine word yielded a 17% interpreter speedup and up to 37% lower peak
RSS). VT Code has no interpreter value union, but it _does_ have enums pushed into `Vec`s in hot paths — token streaming
deltas, per-delta UI events, per-segment rendered markdown. For those types the footprint _is_ the performance.

### VT Code guidelines

- **Box sparse large payloads.** If one variant's payload is much larger than the others (a `String` that is usually
  empty, a `Vec`, a `serde_json::Value`, base64 data), wrap it in `Box<T>` so every other variant stops paying its
  inline size. `SessionMessage` (`vtcode-core/src/utils/session_archive.rs`) and `ThreadItemDetails` / `ThreadEvent`
  (`vtcode-exec-events/src/lib.rs`, 216 → 80 bytes) follow this convention. `Box<T>` is transparent to `serde` and
  `schemars`, so the wire/schema contract is unchanged.
- **Do not box small, hot payloads.** Boxing adds an allocation per value. Variants constructed per streaming delta with
  small payloads (e.g. `AgentMessageItem`) stay inline.
- **Prefer `Option<Box<str>>` / `Box<str>` over `String` for rarely-present or large string fields** in types stored
  per-unit (segments, lines, cells): `Option<Box<str>>` is 16 bytes with a niche vs 24 bytes for `Option<String>`. For
  _small_ string fields (IDs, short names) prefer `CompactStr` per the workspace convention in AGENTS.md — inline
  storage beats both.
- **`#[repr(u8)]` fieldless enums** that are stored in bulk or hashed, so the discriminant never inflates the layout
  (see `vtcode-core/src/tools/registry/circuit_breaker.rs`).
- **Pin the size with a test.** Follow the `size_of` guard convention (e.g.
  `vtcode-exec-events/tests::thread_event_stays_compact`, `vtcode-skills/src/types.rs`:
  `assert!(size_of::<Option<Box<T>>>() < size_of::<Option<T>>())`) so a new variant that balloons a bulk-stored enum
  fails CI instead of silently doubling a queue's memory traffic.
- **Downstream sizes follow automatically.** Queues and budgets keyed on `size_of::<T>()` (e.g. the `QueuedSessionEvent`
  channel budget) shrink with the enum — no separate tuning needed.

## RwLock vs Lock-Free Placement

Per-item `RwLock` in a traversal loop pays two atomic read-modify-writes per element (reader-count
increment on acquire, decrement on release) even with zero writer contention. At 16k elements × 16k
traversals/s that is ~500M atomics/s — the lock bookkeeping, not the data, becomes the bottleneck
([source](https://pranitha.dev/posts/rwlock-vs-lockfree/)).

### VT Code guidelines

- **Never put a `RwLock` around each element of a traversed collection.** Hoist to one coarse lock
  around the whole container when mutations are infrequent (article: coarse `RwLock` around a `Vec`
  with ~1 insert/s still reached ~200k reads/s).
- **For read-heavy / rare-write globals, prefer whole-value `ArcSwap`.** `vtcode-commons`
  `vtcodegitignore` global, `vtcode-indexer` `FileIndexCache` snapshot, and `vtcode-ui` theme runtime
  all follow this: readers do a lock-free `load_full()`, writers `store()` a new `Arc`.
- **Know the alternatives' costs.** Per-item `ArcSwap::load()` still pays per-item overhead versus a
  single epoch `pin()` for a whole traversal; `left-right` (`vtcode-commons::LrMap`) gives wait-free
  reads but doubles memory — only use it when the map fits twice in memory.
- **Match the lock to the context.** `parking_lot` for short sync sections (no poisoning, no async
  yield); `tokio::sync` only in async code and never held across `.await` in a per-item loop.

## Checklist for VT Code Hot Paths

When reviewing or writing a hot path in vtcode:

- [ ] Is there a `.clone()` that could be a reference `&T` instead?
- [ ] Is there a `.clone()` that could be `std::mem::take()` instead?
- [ ] Does the function take `&Vec<T>` or `&String` (should be `&[T]` or `&str`)?
- [ ] Is the error path marked `#[cold]`?
- [ ] Is the small hot function marked `#[inline]`?
- [ ] Is this a large/nested type whose `Debug` is formatted on a hot or fan-out path? Consider
      `#[inline(never)]` (see [Derived trait impls are `#[inline]`](#derived-trait-impls-are-inline)).
- [ ] Does the code use indexed `for i in 0..n` when an iterator would eliminate bounds checks?
- [ ] If a hot loop branches on per-element data, is the predicate unpredictable (~50% selectivity, no pattern)? If so,
      consider [branchless](#branchless-programming-removing-unpredictable-branches) — but run the sorted-vs-shuffled
      diagnostic first, and prefer `memchr` for delimiter scans.
- [ ] Does the code use `Arc<RwLock<T>>` when `&mut T` or `Box<T>` would suffice?
- [ ] Does a traversal loop acquire a `RwLock` per element? Each acquisition pays an atomic read-modify-write
      even without contention — hoist to one coarse lock around the whole container, or swap the whole value
      with `ArcSwap` when writes are rare. See [RwLock vs Lock-Free Placement](#rwlock-vs-lock-free-placement).
- [ ] For read-heavy / rare-write shared state, is whole-value `ArcSwap` (or `LrMap`/`FileIndexCache` snapshot)
      used instead of per-item locks? Prefer `parking_lot` for sync short sections, never hold `tokio::sync`
      locks across `.await` in a per-item loop.
- [ ] Is the type stored in a bulk collection (`Vec`, queue, per-line buffer)? If so, is its enum footprint minimal —
      sparse large payloads boxed, and a `size_of` guard test pinning it? See
      [Enum Footprint](#enum-footprint-in-bulk-collections).
- [ ] Is overflow handling explicit (`checked_*`/`saturating_*`/`wrapping_*`) rather than relying on implicit wrap?
- [ ] Has the performance been measured against baseline before/after?

---

## References

- [r/rust: "Why ISN'T Rust faster than C?" (2024)](https://www.reddit.com/r/rust/comments/1at3r6d/why_isnt_rust_faster_than_c_given_it_can_leverage/)
  — comprehensive discussion covering Fortran as the actual performance champion, noalias bug history, LLVM's C legacy,
  and the cascading-optimization-failure cost of safety checks.
- [r/rust: "What makes Rust faster than C/C++?" (2021)](https://www.reddit.com/r/rust/comments/px72r1/what_makes_rust_faster_than_cc/)
- [Where Rust Really Shines (Manish Goregaokar)](https://manishearth.github.io/blog/2015/05/03/where-rust-really-shines/)
- [The Relative Performance of C and Rust (Bryan Cantrill)](https://blog.oxide.computer/relative-performance-c-rust)
- [Rustc Guide: LLVM noalias](https://rustc-dev-guide.rust-lang.org/backend/misc.html#the-noalias-attribute)
- [Branchless Rust: Making a Filter 4x Faster by Removing an if](https://www.greyblake.com/blog/branchless-rust/) —
  Serhii Potapov, 2026 (source of the Branchless section: misprediction cost, the sorted-vs-shuffled diagnostic, the
  always-write/conditionally-advance transform, and the "trade not magic" caveat).
- [Why is processing a sorted array faster than processing an unsorted array?](https://stackoverflow.com/questions/11227809)
  — Stack Overflow, 27K upvotes (the classic misprediction demo).
- [Mispredicted branches can multiply your running times](https://lemire.me/blog/2019/10/15/mispredicted-branches-can-multiply-your-running-times/)
  — Daniel Lemire.
- [Replacing a Rust Enum with a 64-bit Word](https://pointersgonewild.com/2026-08-25-replacing-a-rust-enum-with-a-64-bit-word/)
  — 2026 (source of the Enum Footprint section: discriminant/alignment padding, boxing sparse payloads, and the measured
  17% speedup / 37% peak-RSS reduction from shrinking a bulk-stored value type).
- [TIL: Rust's derive often implies inline](https://yossarian.net/til/post/rust-s-derive-often-implies-inline/) —
  yossarian.net (source of the Derived trait impls subsection: derive emits `#[inline]`; nested error `Debug` impls
  inline transitively; `uv` reclaimed ~160 KB with an `#[inline(never)]` Debug derive).
- [The Performance Cost of RwLock in Our Read-Heavy Workload](https://pranitha.dev/posts/rwlock-vs-lockfree/) —
  Pranitha Madapathi, 2026 (source of the RwLock vs Lock-Free section: per-item `RwLock` atomic RMW cost,
  single-`pin()` epoch traversal, coarse-lock vs `ArcSwap` vs `left-right` tradeoffs).
- VT Code internal: `docs/development/performance.md`
- VT Code internal: `docs/development/performance-hasher-policy.md`
- VT Code internal: `docs/development/async-performance-audit.md`
