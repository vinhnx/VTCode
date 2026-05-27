# Rust-Specific Performance Principles for VT Code

This document captures the nuance of what makes Rust fast (and where it isn't) in the context of the vtcode project. It complements the general guidelines in `performance.md` by focusing on Rust-specific properties that affect the optimizer, the standard library, and day-to-day coding decisions.

## Table of Contents

- [Core Insight: Rust Is Not Faster Than C/C++ — It Is *Safer While Being Equally Fast*](#core-insight-rust-is-not-faster-than-cc--it-is-safer-while-being-equally-fast)
- [Destructive Move Semantics](#destructive-move-semantics)
- [Aliasing Guarantees (`noalias`)](#aliasing-guarantees-noalias)
- [Immutable by Default & `const` Semantics](#immutable-by-default--const-semantics)
- [Bounds Checking & Iterator Elision](#bounds-checking--iterator-elision)
- [The `#[cold]` and `#[inline]` Strategy](#the-cold-and-inline-strategy)
- [ABI Stability & Standard Library Evolution](#abi-stability--standard-library-evolution)
- [LLVM's C/C++ Legacy: Why Rust's Extra Information Does Not Always Translate](#llvms-cc-legacy-why-rusts-extra-information-does-not-always-translate)
- [Safety Enables Aggressive Optimization](#safety-enables-aggressive-optimization)
- [When Rust Can Be Slower Than C/C++](#when-rust-can-be-slower-than-cc)
- [Checklist for VT Code Hot Paths](#checklist-for-vt-code-hot-paths)

---

## Core Insight: Rust Is Not Faster Than C/C++ — It Is *Safer While Being Equally Fast*

For a well-optimized program, Rust and C++ produce comparable machine code. The performance differences are marginal and situational. The real advantage of Rust is that it makes it *easier* to write fast, correct code without compromising safety. In C++, defensive programming (extra copies, conservative synchronization) erodes performance when engineers are not operating at peak expertise. Rust's type system eliminates the need for much of that defensive overhead.

Note that **C is not the "diamond standard" of performance** — that title arguably belongs to **Fortran**, whose stronger aliasing guarantees (no pointer aliasing at all) have enabled decades of superior numerical optimization. Rust's ownership model places it in a similar position to Fortran: the compiler *knows* references are unique, whereas C requires the explicit `restrict` keyword (rarely used in practice). Rust is structurally positioned to match or exceed C's optimization ceiling, but realizing that potential depends on the backend's ability to consume the information — which brings us to LLVM.

**VT Code implication**: When choosing between a safe and an `unsafe` implementation, prefer the safe one and measure first. The borrow checker gives the optimizer information that C++ cannot express, so safe Rust can *already* produce better code than C++ in many cases.

---

## Destructive Move Semantics

Rust moves are *bitwise*: they copy the bytes and the source is no longer considered valid. In C++, a moved-from object must remain destructible, so the move constructor leaves behind a valid (often empty) state and the destructor still runs. This has two consequences:

1. **No post-move cleanup**: Rust's `Vec::pop`, `String::pop`, `std::mem::take`, and `Option::take` all generate simpler, more optimizable assembly than their C++ counterparts.

2. **Realloc works**: `Vec` can use `realloc` on growth because moves are bitwise. C++ `std::vector` cannot safely `realloc` non-trivial types.

### VT Code guidelines

- Use `std::mem::take(&mut value)` instead of `.clone()` followed by `.clear()` when you need to move a value out of a `&mut` reference.
- Use `Option::take()` for the same pattern with `Option<T>`.
- Prefer `Vec::pop()` over indexed removal when order doesn't matter.
- Use `Vec::drain(..)` instead of manual element-by-element moves for bulk extraction.

**Already applied**: `std::mem::take` is used in 24+ locations across vtcode-core (agent runtime, events, stream buffer, pipeline, etc.). Continue this pattern.

---

## Aliasing Guarantees (`noalias`)

The single biggest theoretical advantage Rust has over C/C++ in the optimizer is pointer aliasing information:

- `&mut T` is guaranteed to be *unique* — no other reference can alias it. This is equivalent to C's `restrict` keyword, applied implicitly to every mutable reference.
- `&T` is guaranteed to be *immutable* — the value cannot mutate while the reference exists.

C++ `const T&` does *not* carry this guarantee: `const_cast` can remove const-ness, and mutable aliases may exist. The optimizer must assume the worst.

### History: Rust as an LLVM bug finder

Rust's aggressive emission of `noalias` has historically been a rollercoaster. The feature was initially enabled around 2014–2015 after Rust settled on `&mut` semantics, then deactivated due to LLVM bugs. It was re-enabled and quickly deactivated again in 2018. Finally, with LLVM 12 (Rust 1.54+), `-Zmutable-noalias=yes` was enabled by default.

Before each deactivation, Rust's `noalias` emission **revealed multiple bugs in LLVM** — bugs that existed but were never triggered because no C/C++ frontend emitted `noalias` as aggressively. In effect, Rust has been a stress-test for LLVM's alias analysis, improving codegen for all LLVM frontends (including Clang). Fortran (via gfortran) similarly exercises GCC's aliasing paths, which is why GCC's handling has historically been more robust — but LLVM's Flang frontend is younger and hasn't yet had the same shake-down.

As of Rust 1.54+ / LLVM 12+, `&mut T` in vtcode gives LLVM *actionable* alias information that C++ cannot express.

### VT Code guidelines

- Prefer `&mut T` over raw pointers to communicate non-aliasing intent.
- When writing hot loops over slices, use `&mut [T]` and `&[T]` rather than `*mut T`/ `*const T` — the optimizer gets alias info for free.
- Use `split_at_mut` for slice subdivisions instead of raw pointer arithmetic.
- Avoid `UnsafeCell` unless profiling proves it necessary — it suppresses alias analysis.

---

## Immutable by Default & `const` Semantics

In C++, `const` can be cast away with `const_cast`, so the optimizer cannot fully trust it. In Rust:
- `&T` is truly immutable (there is no safe `const_cast` equivalent)
- Values are immutable by default; `mut` is explicit

This means the Rust compiler (and LLVM) can cache loaded values across function calls without reloading. In C++, a function receiving `const int&` must reload after every call because the callee might have cast away const.

### VT Code guidelines

- Use `&T` rather than `&mut T` wherever mutation is not needed — it communicates aliasing safety to the optimizer.
- Use `&str` rather than `&String` in function parameters.
- Use `&[T]` rather than `&Vec<T>` in function parameters.
- Make fields `pub` only when needed; prefer immutable public API surfaces.

---

## Bounds Checking & Iterator Elision

Rust performs bounds checking on array/slice indexing by default. In hot loops, this can inhibit vectorization and other optimizations when the compiler cannot prove the bounds.

The real cost of bounds checks is rarely the arithmetic itself — it is the **cascading failure of pattern-matching in the optimizer**. LLVM optimizations are largely pattern-based: if a bounds check creates IR that doesn't match a vectorization or loop-hoisting pattern, the compiler may miss entire families of optimizations downstream. The check itself may add zero measurable cycles, but the optimizations it blocks can cost double-digit percentages.

*However*:
- Iterator patterns (`for x in slice`, `.iter()`, `.iter_mut()`, `.chunks()`) elide bounds checks entirely because the iterator guarantees in-bounds access.
- The optimizer often eliminates bounds checks in `for i in 0..slice.len()` loops.
- `unsafe` is available for the rare cases where the compiler cannot prove safety.

### VT Code guidelines

- Prefer iterator combinators (`map`, `filter`, `fold`, `for_each`) over indexed loops in hot paths.
- Use `for x in &slice` / `for x in &mut slice` instead of `for i in 0..slice.len() { slice[i] ... }`.
- Use `.chunks()` and `.windows()` for sliding-window access to elide per-element bounds checks.
- Only use `unsafe { get_unchecked() }` when profiling proves bounds checks are a bottleneck.

**Measured in vtcode**: Indexed `for i in 0..N` loops are rare in core hot paths (found mostly in tests and memory_pool setup). This is good.

---

## The `#[cold]` and `#[inline]` Strategy

The `#[cold]` attribute tells LLVM that a function is unlikely to be executed. This causes LLVM to:
- Move the cold code to a separate section (improving instruction cache locality for hot paths).
- Not inline the cold function (shrinking hot-path code size).

This is directly analogous to how C++ compilers move exception-handling code to cold sections (GCC `-freorder-blocks-and-partition`).

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

### Where *not* to use `#[inline]`

- Large functions — inlining them bloats code size and pollutes the instruction cache
- Functions only called from one place (LLVM will inline them anyway if profitable)
- Error-only paths (mark these `#[cold]` instead)

### VT Code current state

| Annotation | Count | Assessment |
|---|---|---|
| `#[inline]` | ~150 | Good coverage on hot small functions |
| `#[cold]` | ~75 | Well-covered; most error-diagnostic paths are annotated.

**Action**: When adding new error-only functions, annotate them `#[cold]` rather than `#[inline]`.

---

## ABI Stability & Standard Library Evolution

C++'s standard library is constrained by ABI stability: `std::unordered_map` is locked into a node-based design, `std::regex` cannot switch to a faster implementation, and `std::string` cannot drop its small-string-optimization layout without breaking linked binaries.

Rust has no stable ABI for the standard library. This means:
- `HashMap` in `std` was replaced by `hashbrown` (a Swiss-table implementation) — significantly faster than C++ `std::unordered_map`.
- The standard library can adopt new data structures and algorithms without breaking existing binaries.

### VT Code implications

- vtcode already uses `hashbrown::HashMap` directly (~370 uses) and `rustc_hash::FxHashMap` for measured hotspots. This is correct.
- Unlike C++ projects, vtcode does not need third-party hash map replacements; `hashbrown` is already the best available.
- The `regex` crate (used via dependencies) is already faster than C++ `std::regex` due to its compiled-once, automata-based approach.

---

## LLVM's C/C++ Legacy: Why Rust's Extra Information Does Not Always Translate

Despite Rust's richer semantic information, LLVM — the primary backend for `rustc` — was designed and optimized for C/C++ over two decades. This creates several bottlenecks:

### Niche information is dropped

Rust guarantees niches: `&T` is never null, `&u16` is always 2-byte aligned, `bool` is only 0 or 1, etc. Rust's internal type system tracks these, but LLVM has no first-class concept of niches — C and C++ do not have them. When rustc lowers to LLVM IR, most niche information is either discarded or represented in ways LLVM cannot exploit. Active work exists to improve this, but LLVM's IR was not designed for it.

### No optimized calling convention for sum types

Rust uses `Option<T>` and `Result<T, E>` pervasively. These are tagged unions (discriminant + payload). C has tagged unions too, but no ABI or calling convention optimizes their passing — e.g., passing the discriminant in a flag register and splitting variants across registers vs. stack. Neither GCC nor LLVM support such conventions because C never needed them. This means returning `Result<T, E>` from a function can involve unnecessary memory traffic that a hypothetical optimal calling convention would avoid.

### Move-heavy codegen is less tuned

Rust's pervasive move semantics (bitwise copy + source invalidation) are uncommon in C/C++. When constructing a `Box::new(value)`, Rust constructs the value on the stack then copies it to the heap. LLVM can elide this copy (NRVO-style), but the pattern-matching isn't always successful. Equivalent C code (allocate on heap, initialize in-place) generates simpler IR from the start.

### What this means for vtcode

These are backend limitations, not language limitations. As LLVM evolves (or if Rust gains an alternative backend like GCC or Cranelift), these gaps will narrow. For vtcode's workload (I/O-bound LLM orchestration, not tight numeric loops), these issues are unlikely to be material — but they explain why Rust's "free performance from information" has not materialized at scale.

## Safety Enables Aggressive Optimization

The most practically significant performance difference between Rust and C++ in a real-world project is not compiler optimization — it is the *social and architectural* effect of safety.

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

- When you find yourself adding a `.clone()` to appease the borrow checker in a hot path, consider changing the data structure or ownership model instead. A reference (`&T`) or a move (`std::mem::take`) is usually cheaper.
- Do not reach for `Arc<RwLock<T>>` by default. A `&mut T` or a simple `Box<T>` with exclusive access is faster.
- Use `Rc<T>` for single-threaded shared ownership when the reference is immutable; avoid `Arc` unless cross-thread sharing is proven necessary.

---

## When Rust Can Be Slower Than C/C++

Rust has a few areas where it may be slower:

| Area | Why | Mitigation |
|---|---|---|
| **Floating-point math** | No global `-ffast-math` equivalent in safe Rust. LLVM strict FP semantics prevent many optimizations. | Use `-C llvm-args=-enable-unsafe-fp-math` for measured numeric hot paths, or target-specific intrinsics. |
| **Result checking in tight loops** | `Result<T, E>` is always checked; exceptions in C++ can be truly zero-cost when the sad path is rare. | Use `.unwrap_unchecked()` in `unsafe` blocks where invariants guarantee success (profile first). |
| **Bounds checking** | Default indexing includes bounds checks. | Use iterators or `get_unchecked()` when proven necessary. |
| **Move-heavy heap allocation** | Rust constructs values on the stack then copies to heap (`Box::new(val)`); LLVM does not always elide the intermediate copy. C allocates and initializes in-place. | Use `Box::new_uninit()` + manual init for measured hot paths, or arena allocation patterns. |
| **Panic infrastructure** | Panic unwinding has overhead even if panic never occurs. | Use `panic = "abort"` in release (already vtcode's default). |
| **Compile time** | Not a runtime concern, but Rust's generics and monomorphization increase build times. | Use `codegen-units=1` (already vtcode's release default), `lld` linker, and `-Zshare-generics`. |

For vtcode, none of these are material concerns given the workload characteristics (I/O-bound LLM calls, not tight numeric loops).

---

## Integer Overflow Checking: Near-Zero Cost with Proper Optimization

A common intuition is that checked arithmetic (panicking on overflow) imposes significant runtime cost. Production experience from a former Microsoft Midori team compiler engineer ([source](https://ed0u11h)) demonstrates this is not the case: with proper compiler support, the overhead of overflow checking on **every** arithmetic operation was "literally unmeasurable" for most workloads, and at most 1.2% tax in the worst case.

### Why checked arithmetic is cheap in a well-designed compiler

1. **Late lowering**: The compiler keeps "add with overflow" as a single opcode in its IR throughout all optimization passes. Only at the very end — during machine-code lowering — does it emit the `add` + `jo` (jump on overflow) sequence. This means no optimization is inhibited by the presence of overflow checks — they don't break basic blocks, don't block vectorization, and don't impede code motion.

2. **Range analysis eliminates unnecessary checks**: If the compiler can statically prove an operation cannot overflow (e.g., `(i & 0xFF) + 0x1000` where both operands are bounded), it simply omits the check. This creates a **virtuous cycle**: checked arithmetic constrains the range of values, which lets the compiler eliminate checks on downstream operations, which in turn enables better optimization of subsequent code.

3. **Overflow coalescing**: Expression reassociation reduces the number of checks. `n + 4 + 4` is rewritten to `n + 8`, requiring only one overflow check instead of two. These patterns arise naturally in generated code (e.g., RPC serialization stubs) and compilers that treat checked arithmetic as a first-class optimization target handle them automatically.

4. **Inlining is the amplifier**: Inlining gives range analysis broader visibility into callers' invariants. A function like `checked_add(high, 1)` inlined into a context where `high < MAX` eliminates the check entirely. The tighter the language guarantees, the more the optimizer can eliminate.

### Checked arithmetic enables *better* optimization

A subtle but important point: checked arithmetic makes the optimizer's job **easier**, not harder. When an operation would overflow, all subsequent code is dead (execution jumps to the panic handler). The compiler does not need to consider those states. Compare with C/C++ where signed overflow is undefined behavior — the compiler assumes it never happens, but the programmer cannot assume the same thing. In Rust, overflow is defined behavior (panic in debug, wrap in release), which means the compiler has *more* constraints it can exploit, not fewer.

### What this means for vtcode

vtcode already follows best practices:

| Practice | vtcode status |
|---|---|
| Profile-based overflow control | `overflow-checks = true` in test, `false` in CI/release — correct split |
| Semantic overflow methods | `checked_*` for fallible paths, `saturating_*` for clamping, `wrapping_*` for hashing — all used appropriately |
| Hash code uses `wrapping_mul` | FNV-1a, MurmurHash3 — wrapping is the intended semantics, no checks needed |
| No `unchecked_*` intrinsics | Appropriate — vtcode is I/O-bound, not tight numeric loops |

Guidelines for ongoing work:

- **Do not avoid `checked_*` in hot paths out of performance fear**. The optimizer handles it well. Use it where overflow indicates a real bug.
- **Prefer `wrapping_*` for hash computations** (already done) — this communicates intent and avoids test-mode panics.
- **Use `saturating_*` for UI/cursor/size math** (already done in TUI) — clamping is the correct semantics for layout.
- **Only reach for `unsafe { unchecked_add() }` when profiling proves a bottleneck** — this has not been necessary in vtcode to date.
- **Leverage the test profile**: since `overflow-checks = true` in `[profile.test]`, any arithmetic overflow in tests panics immediately, catching bugs that would silently wrap in release.

The Rust compiler's overflow checking is not yet at the level of the Midori compiler described above (rustc's MIR does not keep overflow-checked ops as single nodes through all optimization passes — LLVM sees the branch). However, the direction of travel is the same, and for vtcode's workload, the cost is already negligible.

---

## Checklist for VT Code Hot Paths

When reviewing or writing a hot path in vtcode:

- [ ] Is there a `.clone()` that could be a reference `&T` instead?
- [ ] Is there a `.clone()` that could be `std::mem::take()` instead?
- [ ] Does the function take `&Vec<T>` or `&String` (should be `&[T]` or `&str`)?
- [ ] Is the error path marked `#[cold]`?
- [ ] Is the small hot function marked `#[inline]`?
- [ ] Does the code use indexed `for i in 0..n` when an iterator would eliminate bounds checks?
- [ ] Does the code use `Arc<RwLock<T>>` when `&mut T` or `Box<T>` would suffice?
- [ ] Is overflow handling explicit (`checked_*`/`saturating_*`/`wrapping_*`) rather than relying on implicit wrap?
- [ ] Has the performance been measured against baseline before/after?

---

## References

- [r/rust: "Why ISN'T Rust faster than C?" (2024)](https://www.reddit.com/r/rust/comments/1at3r6d/why_isnt_rust_faster_than_c_given_it_can_leverage/) — comprehensive discussion covering Fortran as the actual performance champion, noalias bug history, LLVM's C legacy, and the cascading-optimization-failure cost of safety checks.
- [r/rust: "What makes Rust faster than C/C++?" (2021)](https://www.reddit.com/r/rust/comments/px72r1/what_makes_rust_faster_than_cc/)
- [Where Rust Really Shines (Manish Goregaokar)](https://manishearth.github.io/blog/2015/05/03/where-rust-really-shines/)
- [The Relative Performance of C and Rust (Bryan Cantrill)](https://blog.oxide.computer/relative-performance-c-rust)
- [Rustc Guide: LLVM noalias](https://rustc-dev-guide.rust-lang.org/backend/misc.html#the-noalias-attribute)
- VT Code internal: `docs/development/performance.md`
- VT Code internal: `docs/development/performance-hasher-policy.md`
- VT Code internal: `docs/development/async-performance-audit.md`
