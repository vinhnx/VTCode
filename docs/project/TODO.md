[DON'T DELETE UNTIL FEEL COMPLETE] review duplicated and redundent logic from whole code base and remove and cleanup AND DRY.

continue with your recommendation, proceed with outcome. don't stop. review overall progress and changes again carefully, can you do better? go on don't ask me

--


check and fix duplicated status line 


```
iTerm2 | main*                                                                                                                                                                 MiniMaxAI/MiniMax-M2.1:novita | (low)
? help • / command • @ file • # prompt

 Type your message (@files, /commands, ctrl+r: search, Shift+Tab: mode, escape: cancel, tab: queue)

iTerm2 main*                                                                                                                                                            ↕ 100% MiniMaxAI/MiniMax-M2.1:novita | (low)
```

---

```
The previous sections of this book have discussed Rust-specific techniques. This section gives a brief overview of some general performance principles.

As long as the obvious pitfalls are avoided (e.g. [using non-release builds](build-configuration.html)), Rust code generally is fast and uses little memory. Especially if you are used to dynamically-typed languages such as Python and Ruby, or statically-types languages with a garbage collector such as Java and C#.

Optimized code is often more complex and takes more effort to write than unoptimized code. For this reason, it is only worth optimizing hot code.

The biggest performance improvements often come from changes to algorithms or data structures, rather than low-level optimizations.[**Example 1**](https://github.com/rust-lang/rust/pull/53383/commits/5745597e6195fe0591737f242d02350001b6c590),[**Example 2**](https://github.com/rust-lang/rust/pull/54318/commits/154be2c98cf348de080ce951df3f73649e8bb1a6).

Writing code that works well with modern hardware is not always easy, but worth striving for. For example, try to minimize cache misses and branch mispredictions, where possible.

Most optimizations result in small speedups. Although no single small speedup is noticeable, they really add up if you can do enough of them.

Different profilers have different strengths. It is good to use more than one.

When profiling indicates that a function is hot, there are two common ways to speed things up: (a) make the function faster, and/or (b) avoid calling it as much.

It is often easier to eliminate silly slowdowns than it is to introduce clever speedups.

Avoid computing things unless necessary. Lazy/on-demand computations are often a win.[**Example 1**](https://github.com/rust-lang/rust/pull/36592/commits/80a44779f7a211e075da9ed0ff2763afa00f43dc),[**Example 2**](https://github.com/rust-lang/rust/pull/50339/commits/989815d5670826078d9984a3515eeb68235a4687).

Complex general cases can often be avoided by optimistically checking for common special cases that are simpler.[**Example 1**](https://github.com/rust-lang/rust/pull/68790/commits/d62b6f204733d255a3e943388ba99f14b053bf4a),[**Example 2**](https://github.com/rust-lang/rust/pull/53733/commits/130e55665f8c9f078dec67a3e92467853f400250),[**Example 3**](https://github.com/rust-lang/rust/pull/65260/commits/59e41edcc15ed07de604c61876ea091900f73649). In particular, specially handling collections with 0, 1, or 2 elements is often a win when small sizes dominate.[**Example 1**](https://github.com/rust-lang/rust/pull/50932/commits/2ff632484cd8c2e3b123fbf52d9dd39b54a94505),[**Example 2**](https://github.com/rust-lang/rust/pull/64627/commits/acf7d4dcdba4046917c61aab141c1dec25669ce9),[**Example 3**](https://github.com/rust-lang/rust/pull/64949/commits/14192607d38f5501c75abea7a4a0e46349df5b5f),[**Example 4**](https://github.com/rust-lang/rust/pull/64949/commits/d1a7bb36ad0a5932384eac03d3fb834efc0317e5).

Similarly, when dealing with repetitive data, it is often possible to use a simple form of data compression, by using a compact representation for common values and then having a fallback to a secondary table for unusual values.[**Example 1**](https://github.com/rust-lang/rust/pull/54420/commits/b2f25e3c38ff29eebe6c8ce69b8c69243faa440d),[**Example 2**](https://github.com/rust-lang/rust/pull/59693/commits/fd7f605365b27bfdd3cd6763124e81bddd61dd28),[**Example 3**](https://github.com/rust-lang/rust/pull/65750/commits/eea6f23a0ed67fd8c6b8e1b02cda3628fee56b2f).

When code deals with multiple cases, measure case frequencies and handle the most common ones first.

When dealing with lookups that involve high locality, it can be a win to put a small cache in front of a data structure.

Optimized code often has a non-obvious structure, which means that explanatory comments are valuable, particularly those that reference profiling measurements. A comment like “99% of the time this vector has 0 or 1 elements, so handle those cases first” can be illuminating.
```

---

Rust provides excellent support for safe parallel programming, which can lead to large performance improvements. There are a variety of ways to introduce parallelism into a program and the best way for any program will depend greatly on its design.

Having said that, an in-depth treatment of parallelism is beyond the scope of this book.

If you are interested in thread-based parallelism, the documentation for the [`rayon`](https://crates.io/crates/rayon) and [`crossbeam`](https://crates.io/crates/crossbeam) crates is a good place to start. [Rust Atomics and Locks](https://marabos.nl/atomics/) is also an excellent resource.

If you are interested in fine-grained data parallelism, this [blog post](https://shnatsel.medium.com/the-state-of-simd-in-rust-in-2025-32c263e5f53d) is a good overview of the state of SIMD support in Rust as of November 2025.

---

When you have a small piece of very hot code it may be worth inspecting the generated machine code to see if it has any inefficiencies, such as removable [bounds checks](bounds-checks.html). The [Compiler Explorer](https://godbolt.org/) website is an excellent resource when doing this on small snippets. [`cargo-show-asm`](https://github.com/pacak/cargo-show-asm) is an alternative tool that can be used on full Rust projects.

Relatedly, the [`core::arch`](https://doc.rust-lang.org/core/arch/index.html) module provides access to architecture-specific intrinsics, many of which relate to SIMD instructions.

--

Rust has a variety of “wrapper” types, such as [`RefCell`](https://doc.rust-lang.org/std/cell/struct.RefCell.html) and [`Mutex`](https://doc.rust-lang.org/std/sync/struct.Mutex.html), that provide special behavior for values. Accessing these values can take a non-trivial amount of time. If multiple such values are typically accessed together, it may be better to put them within a single wrapper.

For example, a struct like this:

```rust
#![allow(unused)]
fn main() {
use std::sync::{Arc, Mutex};
struct S {
    x: Arc<Mutex<u32>>,
    y: Arc<Mutex<u32>>,
}
}
```

may be better represented like this:

```rust
#![allow(unused)]
fn main() {
use std::sync::{Arc, Mutex};
struct S {
    xy: Arc<Mutex<(u32, u32)>>,
}
}
```

Whether or not this helps performance will depend on the exact access patterns of the values.[**Example**](https://github.com/rust-lang/rust/pull/68694/commits/7426853ba255940b880f2e7f8026d60b94b42404).


---

Sometimes logging code or debugging code can slow down a program significantly. Either the logging/debugging code itself is slow, or data collection code that feeds into logging/debugging code is slow. Make sure that no unnecessary work is done for logging/debugging purposes when logging/debugging is not enabled.[**Example 1**](https://github.com/rust-lang/rust/pull/50246/commits/2e4f66a86f7baa5644d18bb2adc07a8cd1c7409d),[**Example 2**](https://github.com/rust-lang/rust/pull/75133/commits/eeb4b83289e09956e0dda174047729ca87c709fe),[**Example 3**](https://github.com/rust-lang/rust/pull/147293/commits/cb0f969b623a7e12a0d8166c9a498e17a8b5a3c4).

Note that [`assert!`](https://doc.rust-lang.org/std/macro.assert.html) calls always run, but [`debug_assert!`](https://doc.rust-lang.org/std/macro.debug_assert.html) calls only run in dev builds. If you have an assertion that is hot but is not necessary for safety, consider making it a `debug_assert!`.[**Example 1**](https://github.com/rust-lang/rust/pull/58210/commits/f7ed6e18160bc8fccf27a73c05f3935c9e8f672e),[**Example 2**](https://github.com/rust-lang/rust/pull/90746/commits/580d357b5adef605fc731d295ca53ab8532e26fb).


---

Rust’s [`print!`](https://doc.rust-lang.org/std/macro.print.html) and [`println!`](https://doc.rust-lang.org/std/macro.println.html) macros lock stdout on every call. If you have repeated calls to these macros it may be better to lock stdout manually.

For example, change this code:

```rust
#![allow(unused)]
fn main() {
let lines = vec!["one", "two", "three"];
for line in lines {
    println!("{}", line);
}
}
```

to this:

```rust
#![allow(unused)]
fn main() {
fn blah() -> Result<(), std::io::Error> {
let lines = vec!["one", "two", "three"];
use std::io::Write;
let mut stdout = std::io::stdout();
let mut lock = stdout.lock();
for line in lines {
    writeln!(lock, "{}", line)?;
}
// stdout is unlocked when \`lock\` is dropped
Ok(())
}
}
```

stdin and stderr can likewise be locked when doing repeated operations on them.

Rust file I/O is unbuffered by default. If you have many small and repeated read or write calls to a file or network socket, use [`BufReader`](https://doc.rust-lang.org/std/io/struct.BufReader.html) or [`BufWriter`](https://doc.rust-lang.org/std/io/struct.BufWriter.html). They maintain an in-memory buffer for input and output, minimizing the number of system calls required.

For example, change this unbuffered writer code:

```rust
#![allow(unused)]
fn main() {
fn blah() -> Result<(), std::io::Error> {
let lines = vec!["one", "two", "three"];
use std::io::Write;
let mut out = std::fs::File::create("test.txt")?;
for line in lines {
    writeln!(out, "{}", line)?;
}
Ok(())
}
}
```

to this:

```rust
#![allow(unused)]
fn main() {
fn blah() -> Result<(), std::io::Error> {
let lines = vec!["one", "two", "three"];
use std::io::{BufWriter, Write};
let mut out = BufWriter::new(std::fs::File::create("test.txt")?);
for line in lines {
    writeln!(out, "{}", line)?;
}
out.flush()?;
Ok(())
}
}
```

[**Example 1**](https://github.com/rust-lang/rust/pull/93954),[**Example 2**](https://github.com/nnethercote/dhat-rs/pull/22/commits/8c3ae26f1219474ee55c30bc9981e6af2e869be2).

The explicit call to [`flush`](https://doc.rust-lang.org/std/io/trait.Write.html#tymethod.flush) is not strictly necessary, as flushing will happen automatically when `out` is dropped. However, in that case any error that occurs on flushing will be ignored, whereas an explicit flush will make that error explicit.

Forgetting to buffer is more common when writing. Both unbuffered and buffered writers implement the [`Write`](https://doc.rust-lang.org/std/io/trait.Write.html) trait, which means the code for writing to an unbuffered writer and a buffered writer is much the same. In contrast, unbuffered readers implement the [`Read`](https://doc.rust-lang.org/std/io/trait.Read.html) trait but buffered readers implement the [`BufRead`](https://doc.rust-lang.org/std/io/trait.BufRead.html) trait, which means the code for reading from an unbuffered reader and a buffered reader is different. For example, it is difficult to read a file line by line with an unbuffered reader, but it is trivial with a buffered reader by using [`BufRead::read_line`](https://doc.rust-lang.org/std/io/trait.BufRead.html#method.read_line) or [`BufRead::lines`](https://doc.rust-lang.org/std/io/trait.BufRead.html#method.lines). For this reason, it is hard to write an example for readers like the one above for writers, where the before and after versions are so similar.

Finally, note that buffering also works with stdout, so you might want to combine manual locking *and* buffering when making many writes to stdout.

[This section](heap-allocations.html#reading-lines-from-a-file) explains how to avoid excessive allocations when using [`BufRead`](https://doc.rust-lang.org/std/io/trait.BufRead.html) to read a file one line at a time.

The built-in [String](https://doc.rust-lang.org/std/string/struct.String.html) type uses UTF-8 internally, which adds a small, but nonzero overhead caused by UTF-8 validation when you read input into it. If you just want to process input bytes without worrying about UTF-8 (for example if you handle ASCII text), you can use [`BufRead::read_until`](https://doc.rust-lang.org/std/io/trait.BufRead.html#method.read_until).

There are also dedicated crates for reading [byte-oriented lines of data](https://github.com/Freaky/rust-linereader) and working with [byte strings](https://github.com/BurntSushi/bstr).

--

By default, accesses to container types such as slices and vectors involve bounds checks in Rust. These can affect performance, e.g. within hot loops, though less often than you might expect.

There are several safe ways to change code so that the compiler knows about container lengths and can optimize away bounds checks.

- Replace direct element accesses in a loop by using iteration.
- Instead of indexing into a `Vec` within a loop, make a slice of the `Vec` before the loop and then index into the slice within the loop.
- Add assertions on the ranges of index variables.[**Example 1**](https://github.com/rust-random/rand/pull/960/commits/de9dfdd86851032d942eb583d8d438e06085867b),[**Example 2**](https://github.com/image-rs/jpeg-decoder/pull/167/files).

Getting these to work can be tricky. The [Bounds Check Cookbook](https://github.com/Shnatsel/bounds-check-cookbook/) goes into more detail on this topic.

As a last resort, there are the unsafe methods [`get_unchecked`](https://doc.rust-lang.org/std/primitive.slice.html#method.get_unchecked) and [`get_unchecked_mut`](https://doc.rust-lang.org/std/primitive.slice.html#method.get_unchecked_mut).

---


[`Iterator::collect`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.collect) converts an iterator into a collection such as `Vec`, which typically requires an allocation. You should avoid calling `collect` if the collection is then only iterated over again.

For this reason, it is often better to return an iterator type like `impl Iterator<Item=T>` from a function than a `Vec<T>`. Note that sometimes additional lifetimes are required on these return types, as [this blog post](https://blog.katona.me/2019/12/29/Rust-Lifetimes-and-Iterators/) explains.[**Example**](https://github.com/rust-lang/rust/pull/77990/commits/660d8a6550a126797aa66a417137e39a5639451b).

Similarly, you can use [`extend`](https://doc.rust-lang.org/std/iter/trait.Extend.html#tymethod.extend) to extend an existing collection (such as a `Vec`) with an iterator, rather than collecting the iterator into a `Vec` and then using [`append`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.append).

Finally, when you write an iterator it is often worth implementing the [`Iterator::size_hint`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.size_hint) or [`ExactSizeIterator::len`](https://doc.rust-lang.org/std/iter/trait.ExactSizeIterator.html#method.len) method, if possible.`collect` and `extend` calls that use the iterator may then do fewer allocations, because they have advance information about the number of elements yielded by the iterator.

[`chain`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.chain) can be very convenient, but it can also be slower than a single iterator. It may be worth avoiding for hot iterators, if possible.[**Example**](https://github.com/rust-lang/rust/pull/64801/commits/5ca99b750e455e9b5e13e83d0d7886486231e48a).

Similarly, [`filter_map`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.filter_map) may be faster than using [`filter`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.filter) followed by [`map`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.map).

When a chunking iterator is required and the chunk size is known to exactly divide the slice length, use the faster [`slice::chunks_exact`](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.chunks_exact) instead of [`slice::chunks`](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.chunks).

When the chunk size is not known to exactly divide the slice length, it can still be faster to use `slice::chunks_exact` in combination with either [`ChunksExact::remainder`](https://doc.rust-lang.org/stable/std/slice/struct.ChunksExact.html#method.remainder) or manual handling of excess elements.[**Example 1**](https://github.com/johannesvollmer/exrs/pull/173/files),[**Example 2**](https://github.com/johannesvollmer/exrs/pull/175/files).

The same is true for related iterators:

- [`slice::rchunks`](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.rchunks), [`slice::rchunks_exact`](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.rchunks_exact), and [`RChunksExact::remainder`](https://doc.rust-lang.org/stable/std/slice/struct.RChunksExact.html#method.remainder);
- [`slice::chunks_mut`](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.chunks_mut), [`slice::chunks_exact_mut`](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.chunks_exact_mut), and [`ChunksExactMut::into_remainder`](https://doc.rust-lang.org/stable/std/slice/struct.ChunksExactMut.html#method.into_remainder);
- [`slice::rchunks_mut`](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.rchunks_mut), [`slice::rchunks_exact_mut`](https://doc.rust-lang.org/stable/std/primitive.slice.html#method.rchunks_exact_mut), and [`RChunksExactMut::into_remainder`](https://doc.rust-lang.org/stable/std/slice/struct.RChunksExactMut.html#method.into_remainder).

When iterating over collections of small data types, such as integers, it may be better to use `iter().copied()` instead of `iter()`. Whatever consumes that iterator will receive the integers by value instead of by reference, and LLVM may generate better code in that case.[**Example 1**](https://github.com/rust-lang/rust/issues/106539),[**Example 2**](https://github.com/rust-lang/rust/issues/113789).

This is an advanced technique. You might need to check the generated machine code to be certain it is having an effect. See the [Machine Code](machine-code.html) chapter for details on how to do that.

--

It is worth reading through the documentation for common standard library types—such as [`Vec`](https://doc.rust-lang.org/std/vec/struct.Vec.html), [`Option`](https://doc.rust-lang.org/std/option/enum.Option.html), [`Result`](https://doc.rust-lang.org/std/result/enum.Result.html), and [`Rc`](https://doc.rust-lang.org/std/rc/struct.Rc.html) / [`Arc`](https://doc.rust-lang.org/std/sync/struct.Arc.html) —to find interesting functions that can sometimes be used to improve performance.

It is also worth knowing about high-performance alternatives to standard library types, such as [`Mutex`](https://doc.rust-lang.org/std/sync/struct.Mutex.html), [`RwLock`](https://doc.rust-lang.org/std/sync/struct.RwLock.html), [`Condvar`](https://doc.rust-lang.org/std/sync/struct.Condvar.html), and [`Once`](https://doc.rust-lang.org/std/sync/struct.Once.html).

The best way to create a zero-filled `Vec` of length `n` is with `vec![0; n]`. This is simple and probably [as fast or faster](https://github.com/rust-lang/rust/issues/54628) than alternatives, such as using `resize`, `extend`, or anything involving `unsafe`, because it can use OS assistance.

[`Vec::remove`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.remove) removes an element at a particular index and shifts all subsequent elements one to the left, which makes it O(n). [`Vec::swap_remove`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.swap_remove) replaces an element at a particular index with the final element, which does not preserve ordering, but is O(1).

[`Vec::retain`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.retain) efficiently removes multiple items from a `Vec`. There is an equivalent method for other collection types such as `String`, `HashSet`, and `HashMap`.

[`Option::ok_or`](https://doc.rust-lang.org/std/option/enum.Option.html#method.ok_or) converts an `Option` into a `Result`, and is passed an `err` parameter that is used if the `Option` value is `None`. `err` is computed eagerly. If its computation is expensive, you should instead use [`Option::ok_or_else`](https://doc.rust-lang.org/std/option/enum.Option.html#method.ok_or_else), which computes the error value lazily via a closure. For example, this:

```rust
#![allow(unused)]
fn main() {
fn expensive() {}
let o: Option<u32> = None;
let r = o.ok_or(expensive()); // always evaluates \`expensive()\`
}
```

should be changed to this:

```rust
#![allow(unused)]
fn main() {
fn expensive() {}
let o: Option<u32> = None;
let r = o.ok_or_else(|| expensive()); // evaluates \`expensive()\` only when needed
}
```

[**Example**](https://github.com/rust-lang/rust/pull/50051/commits/5070dea2366104fb0b5c344ce7f2a5cf8af176b0).

There are similar alternatives for [`Option::map_or`](https://doc.rust-lang.org/std/option/enum.Option.html#method.map_or), [`Option::unwrap_or`](https://doc.rust-lang.org/std/option/enum.Option.html#method.unwrap_or),[`Result::or`](https://doc.rust-lang.org/std/result/enum.Result.html#method.or), [`Result::map_or`](https://doc.rust-lang.org/std/result/enum.Result.html#method.map_or), and [`Result::unwrap_or`](https://doc.rust-lang.org/std/result/enum.Result.html#method.unwrap_or).

[`Rc::make_mut`](https://doc.rust-lang.org/std/rc/struct.Rc.html#method.make_mut) / [`Arc::make_mut`](https://doc.rust-lang.org/std/sync/struct.Arc.html#method.make_mut) provide clone-on-write semantics. They make a mutable reference to an `Rc` / `Arc`. If the refcount is greater than one, they will `clone` the inner value to ensure unique ownership; otherwise, they will modify the original value. They are not needed often, but they can be extremely useful on occasion.[**Example 1**](https://github.com/rust-lang/rust/pull/65198/commits/3832a634d3aa6a7c60448906e6656a22f7e35628),[**Example 2**](https://github.com/rust-lang/rust/pull/65198/commits/75e0078a1703448a19e25eac85daaa5a4e6e68ac).

The [`parking_lot`](https://crates.io/crates/parking_lot) crate provides alternative implementations of these synchronization types. The APIs and semantics of the `parking_lot` types are similar but not identical to those of the equivalent types in the standard library.

The `parking_lot` versions used to be reliably smaller, faster, and more flexible than those in the standard library, but the standard library versions have greatly improved on some platforms. So you should measure before switching to `parking_lot`.

If you decide to universally use the `parking_lot` types it is easy to accidentally use the standard library equivalents in some places. You can [use Clippy](linting.html#disallowing-types) to avoid this problem.

--

Shrinking oft-instantiated types can help performance.

For example, if memory usage is high, a heap profiler like [DHAT](https://www.valgrind.org/docs/manual/dh-manual.html) can identify the hot allocation points and the types involved. Shrinking these types can reduce peak memory usage, and possibly improve performance by reducing memory traffic and cache pressure.

Furthermore, Rust types that are larger than 128 bytes are copied with `memcpy` rather than inline code. If `memcpy` shows up in non-trivial amounts in profiles, DHAT’s “copy profiling” mode will tell you exactly where the hot `memcpy` calls are and the types involved. Shrinking these types to 128 bytes or less can make the code faster by avoiding `memcpy` calls and reducing memory traffic.

[`std::mem::size_of`](https://doc.rust-lang.org/std/mem/fn.size_of.html) gives the size of a type, in bytes, but often you want to know the exact layout as well. For example, an enum might be surprisingly large due to a single outsized variant.

The `-Zprint-type-sizes` option does exactly this. It isn’t enabled on release versions of rustc, so you’ll need to use a nightly version of rustc. Here is one possible invocation via Cargo:

```rust
RUSTFLAGS=-Zprint-type-sizes cargo +nightly build --release
```

And here is a possible invocation of rustc:

```rust
rustc +nightly -Zprint-type-sizes input.rs
```

It will print out details of the size, layout, and alignment of all types in use. For example, for this type:

```rust
#![allow(unused)]
fn main() {
enum E {
    A,
    B(i32),
    C(u64, u8, u64, u8),
    D(Vec<u32>),
}
}
```

it prints the following, plus information about a few built-in types.

```rust
print-type-size type: \`E\`: 32 bytes, alignment: 8 bytes
print-type-size     discriminant: 1 bytes
print-type-size     variant \`D\`: 31 bytes
print-type-size         padding: 7 bytes
print-type-size         field \`.0\`: 24 bytes, alignment: 8 bytes
print-type-size     variant \`C\`: 23 bytes
print-type-size         field \`.1\`: 1 bytes
print-type-size         field \`.3\`: 1 bytes
print-type-size         padding: 5 bytes
print-type-size         field \`.0\`: 8 bytes, alignment: 8 bytes
print-type-size         field \`.2\`: 8 bytes
print-type-size     variant \`B\`: 7 bytes
print-type-size         padding: 3 bytes
print-type-size         field \`.0\`: 4 bytes, alignment: 4 bytes
print-type-size     variant \`A\`: 0 bytes
```

The output shows the following.

- The size and alignment of the type.
- For enums, the size of the discriminant.
- For enums, the size of each variant (sorted from largest to smallest).
- The size, alignment, and ordering of all fields. (Note that the compiler has reordered variant `C` ’s fields to minimize the size of `E`.)
- The size and location of all padding.

Alternatively, the [top-type-sizes](https://crates.io/crates/top-type-sizes) crate can be used to display the output in a more compact form.

Once you know the layout of a hot type, there are multiple ways to shrink it.

The Rust compiler automatically sorts the fields in struct and enums to minimize their sizes (unless the `#[repr(C)]` attribute is specified), so you do not have to worry about field ordering. But there are other ways to minimize the size of hot types.

If an enum has an outsized variant, consider boxing one or more fields. For example, you could change this type:

```rust
#![allow(unused)]
fn main() {
type LargeType = [u8; 100];
enum A {
    X,
    Y(i32),
    Z(i32, LargeType),
}
}
```

to this:

```rust
#![allow(unused)]
fn main() {
type LargeType = [u8; 100];
enum A {
    X,
    Y(i32),
    Z(Box<(i32, LargeType)>),
}
}
```

This reduces the type size at the cost of requiring an extra heap allocation for the `A::Z` variant. This is more likely to be a net performance win if the `A::Z` variant is relatively rare. The `Box` will also make `A::Z` slightly less ergonomic to use, especially in `match` patterns.[**Example 1**](https://github.com/rust-lang/rust/pull/37445/commits/a920e355ea837a950b484b5791051337cd371f5d),[**Example 2**](https://github.com/rust-lang/rust/pull/55346/commits/38d9277a77e982e49df07725b62b21c423b6428e),[**Example 3**](https://github.com/rust-lang/rust/pull/64302/commits/b972ac818c98373b6d045956b049dc34932c41be),[**Example 4**](https://github.com/rust-lang/rust/pull/64374/commits/2fcd870711ce267c79408ec631f7eba8e0afcdf6),[**Example 5**](https://github.com/rust-lang/rust/pull/64394/commits/7f0637da5144c7435e88ea3805021882f077d50c),[**Example 6**](https://github.com/rust-lang/rust/pull/71942/commits/27ae2f0d60d9201133e1f9ec7a04c05c8e55e665).

It is often possible to shrink types by using smaller integer types. For example, while it is most natural to use `usize` for indices, it is often reasonable to stores indices as `u32`, `u16`, or even `u8`, and then coerce to `usize` at use points.[**Example 1**](https://github.com/rust-lang/rust/pull/49993/commits/4d34bfd00a57f8a8bdb60ec3f908c5d4256f8a9a),[**Example 2**](https://github.com/rust-lang/rust/pull/50981/commits/8d0fad5d3832c6c1f14542ea0be038274e454524).

Rust vectors contain three words: a length, a capacity, and a pointer. If you have a vector that is unlikely to be changed in the future, you can convert it to a *boxed slice* with [`Vec::into_boxed_slice`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.into_boxed_slice). A boxed slice contains only two words, a length and a pointer. Any excess element capacity is dropped, which may cause a reallocation.

```rust
#![allow(unused)]
fn main() {
use std::mem::{size_of, size_of_val};
let v: Vec<u32> = vec![1, 2, 3];
assert_eq!(size_of_val(&v), 3 * size_of::<usize>());

let bs: Box<[u32]> = v.into_boxed_slice();
assert_eq!(size_of_val(&bs), 2 * size_of::<usize>());
}
```

Alternatively, a boxed slice can be constructed directly from an iterator with [`Iterator::collect`](https://doc.rust-lang.org/std/iter/trait.Iterator.html#method.collect). If the iterator’s length is known in advance, this avoids any reallocation.

```rust
#![allow(unused)]
fn main() {
let bs: Box<[u32]> = (1..3).collect();
}
```

A boxed slice can be converted to a vector with [`slice::into_vec`](https://doc.rust-lang.org/std/primitive.slice.html#method.into_vec) without any cloning or reallocation.

An alternative to boxed slices is `ThinVec`, from the [`thin_vec`](https://crates.io/crates/thin-vec) crate. It is functionally equivalent to `Vec`, but stores the length and capacity in the same allocation as the elements (if there are any). This means that `size_of::<ThinVec<T>>` is only one word.

`ThinVec` is a good choice within oft-instantiated types for vectors that are often empty. It can also be used to shrink the largest variant of an enum, if that variant contains a `Vec`.

If a type is hot enough that its size can affect performance, it is a good idea to use a static assertion to ensure that it does not accidentally regress. The following example uses a macro from the [`static_assertions`](https://crates.io/crates/static_assertions) crate.

```rust
// This type is used a lot. Make sure it doesn't unintentionally get bigger.
  #[cfg(target_arch = "x86_64")]
  static_assertions::assert_eq_size!(HotType, [u8; 64]);
```

The `cfg` attribute is important, because type sizes can vary on different platforms. Restricting the assertion to `x86_64` (which is typically the most widely-used platform) is likely to be good enough to prevent regressions in practice.

---

Heap allocations are moderately expensive. The exact details depend on which allocator is in use, but each allocation (and deallocation) typically involves acquiring a global lock, doing some non-trivial data structure manipulation, and possibly executing a system call. Small allocations are not necessarily cheaper than large allocations. It is worth understanding which Rust data structures and operations cause allocations, because avoiding them can greatly improve performance.

The [Rust Container Cheat Sheet](https://docs.google.com/presentation/d/1q-c7UAyrUlM-eZyTo1pd8SZ0qwA_wYxmPZVOQkoDmH4/) has visualizations of common Rust types, and is an excellent companion to the following sections.

If a general-purpose profiler shows `malloc`, `free`, and related functions as hot, then it is likely worth trying to reduce the allocation rate and/or using an alternative allocator.

[DHAT](https://www.valgrind.org/docs/manual/dh-manual.html) is an excellent profiler to use when reducing allocation rates. It works on Linux and some other Unixes. It precisely identifies hot allocation sites and their allocation rates. Exact results will vary, but experience with rustc has shown that reducing allocation rates by 10 allocations per million instructions executed can have measurable performance improvements (e.g. ~1%).

Here is some example output from DHAT.

```rust
AP 1.1/25 (2 children) {
  Total:     54,533,440 bytes (4.02%, 2,714.28/Minstr) in 458,839 blocks (7.72%, 22.84/Minstr), avg size 118.85 bytes, avg lifetime 1,127,259,403.64 instrs (5.61% of program duration)
  At t-gmax: 0 bytes (0%) in 0 blocks (0%), avg size 0 bytes
  At t-end:  0 bytes (0%) in 0 blocks (0%), avg size 0 bytes
  Reads:     15,993,012 bytes (0.29%, 796.02/Minstr), 0.29/byte
  Writes:    20,974,752 bytes (1.03%, 1,043.97/Minstr), 0.38/byte
  Allocated at {
    #1: 0x95CACC9: alloc (alloc.rs:72)
    #2: 0x95CACC9: alloc (alloc.rs:148)
    #3: 0x95CACC9: reserve_internal<syntax::tokenstream::TokenStream,alloc::alloc::Global> (raw_vec.rs:669)
    #4: 0x95CACC9: reserve<syntax::tokenstream::TokenStream,alloc::alloc::Global> (raw_vec.rs:492)
    #5: 0x95CACC9: reserve<syntax::tokenstream::TokenStream> (vec.rs:460)
    #6: 0x95CACC9: push<syntax::tokenstream::TokenStream> (vec.rs:989)
    #7: 0x95CACC9: parse_token_trees_until_close_delim (tokentrees.rs:27)
    #8: 0x95CACC9: syntax::parse::lexer::tokentrees::<impl syntax::parse::lexer::StringReader<'a>>::parse_token_tree (tokentrees.rs:81)
  }
}
```

It is beyond the scope of this book to describe everything in this example, but it should be clear that DHAT gives a wealth of information about allocations, such as where and how often they happen, how big they are, how long they live for, and how often they are accessed.

[`Box`](https://doc.rust-lang.org/std/boxed/struct.Box.html) is the simplest heap-allocated type. A `Box<T>` value is a `T` value that is allocated on the heap.

It is sometimes worth boxing one or more fields in a struct or enum fields to make a type smaller. (See the [Type Sizes](type-sizes.html) chapter for more about this.)

Other than that, `Box` is straightforward and does not offer much scope for optimizations.

[`Rc`](https://doc.rust-lang.org/std/rc/struct.Rc.html) / [`Arc`](https://doc.rust-lang.org/std/sync/struct.Arc.html) are similar to `Box`, but the value on the heap is accompanied by two reference counts. They allow value sharing, which can be an effective way to reduce memory usage.

However, if used for values that are rarely shared, they can increase allocation rates by heap allocating values that might otherwise not be heap-allocated.[**Example**](https://github.com/rust-lang/rust/pull/37373/commits/c440a7ae654fb641e68a9ee53b03bf3f7133c2fe).

Unlike `Box`, calling `clone` on an `Rc` / `Arc` value does not involve an allocation. Instead, it merely increments a reference count.

[`Vec`](https://doc.rust-lang.org/std/vec/struct.Vec.html) is a heap-allocated type with a great deal of scope for optimizing the number of allocations, and/or minimizing the amount of wasted space. To do this requires understanding how its elements are stored.

A `Vec` contains three words: a length, a capacity, and a pointer. The pointer will point to heap-allocated memory if the capacity is nonzero and the element size is nonzero; otherwise, it will not point to allocated memory.

Even if the `Vec` itself is not heap-allocated, the elements (if present and nonzero-sized) always will be. If nonzero-sized elements are present, the memory holding those elements may be larger than necessary, providing space for additional future elements. The number of elements present is the length, and the number of elements that could be held without reallocating is the capacity.

When the vector needs to grow beyond its current capacity, the elements will be copied into a larger heap allocation, and the old heap allocation will be freed.

A new, empty `Vec` created by the common means ([`vec![]`](https://doc.rust-lang.org/std/macro.vec.html) or [`Vec::new`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.new) or [`Vec::default`](https://doc.rust-lang.org/std/default/trait.Default.html#tymethod.default)) has a length and capacity of zero, and no heap allocation is required. If you repeatedly push individual elements onto the end of the `Vec`, it will periodically reallocate. The growth strategy is not specified, but at the time of writing it uses a quasi-doubling strategy resulting in the following capacities: 0, 4, 8, 16, 32, 64, and so on. (It skips directly from 0 to 4, instead of going via 1 and 2, because this [avoids many allocations](https://github.com/rust-lang/rust/pull/72227) in practice.) As a vector grows, the frequency of reallocations will decrease exponentially, but the amount of possibly-wasted excess capacity will increase exponentially.

This growth strategy is typical for growable data structures and reasonable in the general case, but if you know in advance the likely length of a vector you can often do better. If you have a hot vector allocation site (e.g. a hot [`Vec::push`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.push) call), it is worth using [`eprintln!`](https://doc.rust-lang.org/std/macro.eprintln.html) to print the vector length at that site and then doing some post-processing (e.g. with [`counts`](https://github.com/nnethercote/counts/)) to determine the length distribution. For example, you might have many short vectors, or you might have a smaller number of very long vectors, and the best way to optimize the allocation site will vary accordingly.

If you have many short vectors, you can use the `SmallVec` type from the [`smallvec`](https://crates.io/crates/smallvec) crate. `SmallVec<[T; N]>` is a drop-in replacement for `Vec` that can store `N` elements within the `SmallVec` itself, and then switches to a heap allocation if the number of elements exceeds that. (Note also that `vec![]` literals must be replaced with `smallvec![]` literals.) [**Example 1**](https://github.com/rust-lang/rust/pull/50565/commits/78262e700dc6a7b57e376742f344e80115d2d3f2),[**Example 2**](https://github.com/rust-lang/rust/pull/55383/commits/526dc1421b48e3ee8357d58d997e7a0f4bb26915).

`SmallVec` reliably reduces the allocation rate when used appropriately, but its use does not guarantee improved performance. It is slightly slower than `Vec` for normal operations because it must always check if the elements are heap-allocated or not. Also, If `N` is high or `T` is large, then the `SmallVec<[T; N]>` itself can be larger than `Vec<T>`, and copying of `SmallVec` values will be slower. As always, benchmarking is required to confirm that an optimization is effective.

If you have many short vectors *and* you precisely know their maximum length,`ArrayVec` from the [`arrayvec`](https://crates.io/crates/arrayvec) crate is a better choice than `SmallVec`. It does not require the fallback to heap allocation, which makes it a little faster.[**Example**](https://github.com/rust-lang/rust/pull/74310/commits/c492ca40a288d8a85353ba112c4d38fe87ef453e).

If you know the minimum or exact size of a vector, you can reserve a specific capacity with [`Vec::with_capacity`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.with_capacity), [`Vec::reserve`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.reserve), or [`Vec::reserve_exact`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.reserve_exact). For example, if you know a vector will grow to have at least 20 elements, these functions can immediately provide a vector with a capacity of at least 20 using a single allocation, whereas pushing the items one at a time would result in four allocations (for capacities of 4, 8, 16, and 32).[**Example**](https://github.com/rust-lang/rust/pull/77990/commits/a7f2bb634308a5f05f2af716482b67ba43701681).

If you know the maximum length of a vector, the above functions also let you not allocate excess space unnecessarily. Similarly, [`Vec::shrink_to_fit`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.shrink_to_fit) can be used to minimize wasted space, but note that it may cause a reallocation.

A [`String`](https://doc.rust-lang.org/std/string/struct.String.html) contains heap-allocated bytes. The representation and operation of `String` are very similar to that of `Vec<u8>`. Many `Vec` methods relating to growth and capacity have equivalents for `String`, such as [`String::with_capacity`](https://doc.rust-lang.org/std/string/struct.String.html#method.with_capacity).

The `SmallString` type from the [`smallstr`](https://crates.io/crates/smallstr) crate is similar to the `SmallVec` type.

The `String` type from the [`smartstring`](https://crates.io/crates/smartstring) crate is a drop-in replacement for `String` that avoids heap allocations for strings with less than three words’ worth of characters. On 64-bit platforms, this is any string that is less than 24 bytes, which includes all strings containing 23 or fewer ASCII characters.[**Example**](https://github.com/djc/topfew-rs/commit/803fd566e9b889b7ba452a2a294a3e4df76e6c4c).

Note that the `format!` macro produces a `String`, which means it performs an allocation. If you can avoid a `format!` call by using a string literal, that will avoid this allocation.[**Example**](https://github.com/rust-lang/rust/pull/55905/commits/c6862992d947331cd6556f765f6efbde0a709cf9).[`std::format_args`](https://doc.rust-lang.org/std/macro.format_args.html) and/or the [`lazy_format`](https://crates.io/crates/lazy_format) crate may help with this.

[`HashSet`](https://doc.rust-lang.org/std/collections/struct.HashSet.html) and [`HashMap`](https://doc.rust-lang.org/std/collections/struct.HashMap.html) are hash tables. Their representation and operations are similar to those of `Vec`, in terms of allocations: they have a single contiguous heap allocation, holding keys and values, which is reallocated as necessary as the table grows. Many `Vec` methods relating to growth and capacity have equivalents for `HashSet` / `HashMap`, such as [`HashSet::with_capacity`](https://doc.rust-lang.org/std/collections/struct.HashSet.html#method.with_capacity).

Calling [`clone`](https://doc.rust-lang.org/std/clone/trait.Clone.html#tymethod.clone) on a value that contains heap-allocated memory typically involves additional allocations. For example, calling `clone` on a non-empty `Vec` requires a new allocation for the elements (but note that the capacity of the new `Vec` might not be the same as the capacity of the original `Vec`). The exception is `Rc` / `Arc`, where a `clone` call just increments the reference count.

[`clone_from`](https://doc.rust-lang.org/std/clone/trait.Clone.html#method.clone_from) is an alternative to `clone`. `a.clone_from(&b)` is equivalent to `a = b.clone()` but may avoid unnecessary allocations. For example, if you want to clone one `Vec` over the top of an existing `Vec`, the existing `Vec` ’s heap allocation will be reused if possible, as the following example shows.

```rust
#![allow(unused)]
fn main() {
let mut v1: Vec<u32> = Vec::with_capacity(99);
let v2: Vec<u32> = vec![1, 2, 3];
v1.clone_from(&v2); // v1's allocation is reused
assert_eq!(v1.capacity(), 99);
}
```

Although `clone` usually causes allocations, it is a reasonable thing to use in many circumstances and can often make code simpler. Use profiling data to see which `clone` calls are hot and worth taking the effort to avoid.

Sometimes Rust code ends up containing unnecessary `clone` calls, due to (a) programmer error, or (b) changes in the code that render previously-necessary `clone` calls unnecessary. If you see a hot `clone` call that does not seem necessary, sometimes it can simply be removed.[**Example 1**](https://github.com/rust-lang/rust/pull/37318/commits/e382267cfb9133ef12d59b66a2935ee45b546a61),[**Example 2**](https://github.com/rust-lang/rust/pull/37705/commits/11c1126688bab32f76dbe1a973906c7586da143f),[**Example 3**](https://github.com/rust-lang/rust/pull/64302/commits/36b37e22de92b584b9cf4464ed1d4ad317b798be).

[`ToOwned::to_owned`](https://doc.rust-lang.org/std/borrow/trait.ToOwned.html#tymethod.to_owned) is implemented for many common types. It creates owned data from borrowed data, usually by cloning, and therefore often causes heap allocations. For example, it can be used to create a `String` from a `&str`.

Sometimes `to_owned` calls (and related calls such as `clone` and `to_string`) can be avoided by storing a reference to borrowed data in a struct rather than an owned copy. This requires lifetime annotations on the struct, complicating the code, and should only be done when profiling and benchmarking shows that it is worthwhile.[**Example**](https://github.com/rust-lang/rust/pull/50855/commits/6872377357dbbf373cfd2aae352cb74cfcc66f34).

Sometimes code deals with a mixture of borrowed and owned data. Imagine a vector of error messages, some of which are static string literals and some of which are constructed with `format!`. The obvious representation is `Vec<String>`, as the following example shows.

```rust
#![allow(unused)]
fn main() {
let mut errors: Vec<String> = vec![];
errors.push("something went wrong".to_string());
errors.push(format!("something went wrong on line {}", 100));
}
```

That requires a `to_string` call to promote the static string literal to a `String`, which incurs an allocation.

Instead you can use the [`Cow`](https://doc.rust-lang.org/std/borrow/enum.Cow.html) type, which can hold either borrowed or owned data. A borrowed value `x` is wrapped with `Cow::Borrowed(x)`, and an owned value `y` is wrapped with `Cow::Owned(y)`. `Cow` also implements the `From<T>` trait for various string, slice, and path types, so you can usually use `into` as well. (Or `Cow::from`, which is longer but results in more readable code, because it makes the type clearer.) The following example puts all this together.

```rust
#![allow(unused)]
fn main() {
use std::borrow::Cow;
let mut errors: Vec<Cow<'static, str>> = vec![];
errors.push(Cow::Borrowed("something went wrong"));
errors.push(Cow::Owned(format!("something went wrong on line {}", 100)));
errors.push(Cow::from("something else went wrong"));
errors.push(format!("something else went wrong on line {}", 101).into());
}
```

`errors` now holds a mixture of borrowed and owned data without requiring any extra allocations. This example involves `&str` / `String`, but other pairings such as `&[T]` / `Vec<T>` and `&Path` / `PathBuf` are also possible.

[**Example 1**](https://github.com/rust-lang/rust/pull/37064/commits/b043e11de2eb2c60f7bfec5e15960f537b229e20),[**Example 2**](https://github.com/rust-lang/rust/pull/56336/commits/787959c20d062d396b97a5566e0a766d963af022).

All of the above applies if the data is immutable. But `Cow` also allows borrowed data to be promoted to owned data if it needs to be mutated.[`Cow::to_mut`](https://doc.rust-lang.org/std/borrow/enum.Cow.html#method.to_mut) will obtain a mutable reference to an owned value, cloning if necessary. This is called “clone-on-write”, which is where the name `Cow` comes from.

This clone-on-write behaviour is useful when you have some borrowed data, such as a `&str`, that is mostly read-only but occasionally needs to be modified.

[**Example 1**](https://github.com/rust-lang/rust/pull/50855/commits/ad471452ba6fbbf91ad566dc4bdf1033a7281811),[**Example 2**](https://github.com/rust-lang/rust/pull/68848/commits/67da45f5084f98eeb20cc6022d68788510dc832a).

Finally, because `Cow` implements [`Deref`](https://doc.rust-lang.org/std/ops/trait.Deref.html), you can call methods directly on the data it encloses.

`Cow` can be fiddly to get working, but it is often worth the effort.

Sometimes you need to build up a collection such as a `Vec` in stages. It is usually better to do this by modifying a single `Vec` than by building multiple `Vec` s and then combining them.

For example, if you have a function `do_stuff` that produces a `Vec` that might be called multiple times:

```rust
#![allow(unused)]
fn main() {
fn do_stuff(x: u32, y: u32) -> Vec<u32> {
    vec![x, y]
}
}
```

It might be better to instead modify a passed-in `Vec`:

```rust
#![allow(unused)]
fn main() {
fn do_stuff(x: u32, y: u32, vec: &mut Vec<u32>) {
    vec.push(x);
    vec.push(y);
}
}
```

Sometimes it is worth keeping around a “workhorse” collection that can be reused. For example, if a `Vec` is needed for each iteration of a loop, you could declare the `Vec` outside the loop, use it within the loop body, and then call [`clear`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.clear) at the end of the loop body (to empty the `Vec` without affecting its capacity). This avoids allocations at the cost of obscuring the fact that each iteration’s usage of the `Vec` is unrelated to the others.[**Example 1**](https://github.com/rust-lang/rust/pull/77990/commits/45faeb43aecdc98c9e3f2b24edf2ecc71f39d323),[**Example 2**](https://github.com/rust-lang/rust/pull/51870/commits/b0c78120e3ecae5f4043781f7a3f79e2277293e7).

Similarly, it is sometimes worth keeping a workhorse collection within a struct, to be reused in one or more methods that are called repeatedly.

[`BufRead::lines`](https://doc.rust-lang.org/stable/std/io/trait.BufRead.html#method.lines) makes it easy to read a file one line at a time:

```rust
#![allow(unused)]
fn main() {
fn blah() -> Result<(), std::io::Error> {
fn process(_: &str) {}
use std::io::{self, BufRead};
let mut lock = io::stdin().lock();
for line in lock.lines() {
    process(&line?);
}
Ok(())
}
}
```

But the iterator it produces returns `io::Result<String>`, which means it allocates for every line in the file.

An alternative is to use a workhorse `String` in a loop over [`BufRead::read_line`](https://doc.rust-lang.org/stable/std/io/trait.BufRead.html#method.read_line):

```rust
#![allow(unused)]
fn main() {
fn blah() -> Result<(), std::io::Error> {
fn process(_: &str) {}
use std::io::{self, BufRead};
let mut lock = io::stdin().lock();
let mut line = String::new();
while lock.read_line(&mut line)? != 0 {
    process(&line);
    line.clear();
}
Ok(())
}
}
```

This reduces the number of allocations to at most a handful, and possibly just one. (The exact number depends on how many times `line` needs to be reallocated, which depends on the distribution of line lengths in the file.)

This will only work if the loop body can operate on a `&str`, rather than a `String`.

[**Example**](https://github.com/nnethercote/counts/commit/7d39bbb1867720ef3b9799fee739cd717ad1539a).

It is also possible to improve heap allocation performance without changing your code, simply by using a different allocator. See the [Alternative Allocators](build-configuration.html#alternative-allocators) section for details.

To ensure the number and/or size of allocations done by your code doesn’t increase unintentionally, you can use the *heap usage testing* feature of [dhat-rs](https://crates.io/crates/dhat) to write tests that check particular code snippets allocate the expected amount of heap memory.

---

`HashSet` and `HashMap` are two widely-used types and there are ways to make them faster.

The default hashing algorithm is not specified, but at the time of writing the default is an algorithm called [SipHash 1-3](https://en.wikipedia.org/wiki/SipHash). This algorithm is high quality—it provides high protection against collisions—but is relatively slow, particularly for short keys such as integers.

If profiling shows that hashing is hot, and [HashDoS attacks](https://en.wikipedia.org/wiki/Collision_attack) are not a concern for your application, the use of hash tables with faster hash algorithms can provide large speed wins.

- [`rustc-hash`](https://crates.io/crates/rustc-hash) provides `FxHashSet` and `FxHashMap` types that are drop-in replacements for `HashSet` and `HashMap`. Its hashing algorithm is low-quality but very fast, especially for integer keys, and has been found to out-perform all other hash algorithms within rustc. ([`fxhash`](https://crates.io/crates/fxhash) is an older, less well maintained implementation of the same algorithm and types.)
- [`fnv`](https://crates.io/crates/fnv) provides `FnvHashSet` and `FnvHashMap` types. Its hashing algorithm is higher quality than `rustc-hash` ’s but a little slower.
- [`ahash`](https://crates.io/crates/ahash) provides `AHashSet` and `AHashMap`. Its hashing algorithm can take advantage of AES instruction support that is available on some processors.

If hashing performance is important in your program, it is worth trying more than one of these alternatives. For example, the following results were seen in rustc.

- The switch from `fnv` to `fxhash` gave [speedups of up to 6%](https://github.com/rust-lang/rust/pull/37229/commits/00e48affde2d349e3b3bfbd3d0f6afb5d76282a7).
- An attempt to switch from `fxhash` to `ahash` resulted in [slowdowns of 1-4%](https://github.com/rust-lang/rust/issues/69153#issuecomment-589504301).
- An attempt to switch from `fxhash` back to the default hasher resulted in [slowdowns ranging from 4-84%](https://github.com/rust-lang/rust/issues/69153#issuecomment-589338446)!

If you decide to universally use one of the alternatives, such as `FxHashSet` / `FxHashMap`, it is easy to accidentally use `HashSet` / `HashMap` in some places. You can [use Clippy](linting.html#disallowing-types) to avoid this problem.

Some types don’t need hashing. For example, you might have a newtype that wraps an integer and the integer values are random, or close to random. For such a type, the distribution of the hashed values won’t be that different to the distribution of the values themselves. In this case the [`nohash_hasher`](https://crates.io/crates/nohash-hasher) crate can be useful.

Hash function design is a complex topic and is beyond the scope of this book. The [`ahash` documentation](https://github.com/tkaitchuck/aHash/blob/master/compare/readme.md) has a good discussion.

When you annotate a type with `#[derive(Hash)]` the generated `hash` method will hash each field separately. For some hash functions it may be faster to convert the type to raw bytes and hash the bytes as a stream. This is possible for types that satisfy certain properties such as having no padding bytes.

The [`zerocopy`](https://crates.io/crates/zerocopy) and [`bytemuck`](https://crates.io/crates/bytemuck) crates both provide a `#[derive(ByteHash)]` macro that generates a `hash` method that does this kind of byte-wise hashing. The README for the [`derive_hash_fast`](https://crates.io/crates/derive_hash_fast) crate provides more detail for this technique.

This is an advanced technique, and the performance effects are highly dependent on the hash function and the exact structure of the types being hashed. Measure carefully.

---

Entry to and exit from hot, uninlined functions often accounts for a non-trivial fraction of execution time. Inlining these functions removes these entries and exits and can enable additional low-level optimizations by the compiler. In the best case the overall effect is small but easy speed wins.

There are four inline attributes that can be used on Rust functions.

- **None**. The compiler will decide itself if the function should be inlined. This will depend on factors such as the optimization level, the size of the function, whether the function is generic, and if the inlining is across a crate boundary.
- **`#[inline]`**. This suggests that the function should be inlined.
- **`#[inline(always)]`**. This strongly suggests that the function should be inlined.
- **`#[inline(never)]`**. This strongly suggests that the function should not be inlined.

Inline attributes do not guarantee that a function is inlined or not inlined, but in practice `#[inline(always)]` will cause inlining in all but the most exceptional cases.

Inlining is non-transitive. If a function `f` calls a function `g` and you want both functions to be inlined together at a callsite to `f`, both functions should be marked with an inline attribute.

The best candidates for inlining are (a) functions that are very small, or (b) functions that have a single call site. The compiler will often inline these functions itself even without an inline attribute. But the compiler cannot always make the best choices, so attributes are sometimes needed.[**Example 1**](https://github.com/rust-lang/rust/pull/37083/commits/6a4bb35b70862f33ac2491ffe6c55fb210c8490d),[**Example 2**](https://github.com/rust-lang/rust/pull/50407/commits/e740b97be699c9445b8a1a7af6348ca2d4c460ce),[**Example 3**](https://github.com/rust-lang/rust/pull/50564/commits/77c40f8c6f8cc472f6438f7724d60bf3b7718a0c),[**Example 4**](https://github.com/rust-lang/rust/pull/57719/commits/92fd6f9d30d0b6b4ecbcf01534809fb66393f139),[**Example 5**](https://github.com/rust-lang/rust/pull/69256/commits/e761f3af904b3c275bdebc73bb29ffc45384945d).

Cachegrind is a good profiler for determining if a function is inlined. When looking at Cachegrind’s output, you can tell that a function has been inlined if (and only if) its first and last lines are *not* marked with event counts. For example:

```rust
.  #[inline(always)]
      .  fn inlined(x: u32, y: u32) -> u32 {
700,000      eprintln!("inlined: {} + {}", x, y);
200,000      x + y
      .  }
      .  
      .  #[inline(never)]
400,000  fn not_inlined(x: u32, y: u32) -> u32 {
700,000      eprintln!("not_inlined: {} + {}", x, y);
200,000      x + y
200,000  }
```

You should measure again after adding inline attributes, because the effects can be unpredictable. Sometimes it has no effect because a nearby function that was previously inlined no longer is. Sometimes it slows the code down. Inlining can also affect compile times, especially cross-crate inlining which involves duplicating internal representations of the functions.

Sometimes you have a function that is large and has multiple call sites, but only one call site is hot. You would like to inline the hot call site for speed, but not inline the cold call sites to avoid unnecessary code bloat. The way to handle this is to split the function always-inlined and never-inlined variants, with the latter calling the former.

For example, this function:

```rust
#![allow(unused)]
fn main() {
fn one() {};
fn two() {};
fn three() {};
fn my_function() {
    one();
    two();
    three();
}
}
```

Would become these two functions:

```rust
#![allow(unused)]
fn main() {
fn one() {};
fn two() {};
fn three() {};
// Use this at the hot call site.
#[inline(always)]
fn inlined_my_function() {
    one();
    two();
    three();
}

// Use this at the cold call sites.
#[inline(never)]
fn uninlined_my_function() {
    inlined_my_function();
}
}
```

[**Example 1**](https://github.com/rust-lang/rust/pull/53513/commits/b73843f9422fb487b2d26ac2d65f79f73a4c9ae3),[**Example 2**](https://github.com/rust-lang/rust/pull/64420/commits/a2261ad66400c3145f96ebff0d9b75e910fa89dd).

The inverse of inlining is *outlining*: moving rarely executed code into a separate function. You can add a `#[cold]` attribute to such functions to tell the compiler that the function is rarely called. This can result in better code generation for the hot path.[**Example 1**](https://github.com/Lokathor/tinyvec/pull/127),[**Example 2**](https://crates.io/crates/fast_assert).

---

[Clippy](https://github.com/rust-lang/rust-clippy) is a collection of lints to catch common mistakes in Rust code. It is an excellent tool to run on Rust code in general. It can also help with performance, because a number of the lints relate to code patterns that can cause sub-optimal performance.

Given that automated detection of problems is preferable to manual detection, the rest of this book will not mention performance problems that Clippy detects by default.

Once installed, it is easy to run:

```rust
cargo clippy
```

The full list of performance lints can be seen by visiting the [lint list](https://rust-lang.github.io/rust-clippy/master/) and deselecting all the lint groups except for “Perf”.

As well as making the code faster, the performance lint suggestions usually result in code that is simpler and more idiomatic, so they are worth following even for code that is not executed frequently.

Conversely, some non-performance lint suggestions can improve performance. For example, the [`ptr_arg`](https://rust-lang.github.io/rust-clippy/master/index.html#ptr_arg) style lint suggests changing various container arguments to slices, such as changing `&mut Vec<T>` arguments to `&mut [T]`. The primary motivation here is that a slice gives a more flexible API, but it may also result in faster code due to less indirection and better optimization opportunities for the compiler.[**Example**](https://github.com/fschutt/fastblur/pull/3/files).

In the following chapters we will see that it is sometimes worth avoiding certain standard library types in favour of alternatives that are faster. If you decide to use these alternatives, it is easy to accidentally use the standard library types in some places by mistake.

You can use Clippy’s [`disallowed_types`](https://rust-lang.github.io/rust-clippy/master/index.html#disallowed_types) lint to avoid this problem. For example, to disallow the use of the standard hash tables (for reasons explained in the [Hashing](hashing.html) section) add a `clippy.toml` file to your code with the following line.

```toml
disallowed-types = ["std::collections::HashMap", "std::collections::HashSet"]
```

---

You can drastically change the performance of a Rust program without changing its code, just by changing its build configuration. There are many possible build configurations for each Rust program. The one chosen will affect several characteristics of the compiled code, such as compile times, runtime speed, memory use, binary size, debuggability, profilability, and which architectures your compiled program will run on.

Most configuration choices will improve one or more characteristics while worsening one or more others. For example, a common trade-off is to accept worse compile times in exchange for higher runtime speeds. The right choice for your program depends on your needs and the specifics of your program, and performance-related choices (which is most of them) should be validated with benchmarking.

It is worth reading this chapter carefully to understand all the build configuration choices. However, for the impatient or forgetful,[`cargo-wizard`](https://github.com/Kobzol/cargo-wizard) encapsulates this information and can help you choose an appropriate build configuration.

Note that Cargo only looks at the profile settings in the `Cargo.toml` file at the root of the workspace. Profile settings defined in dependencies are ignored. Therefore, these options are mostly relevant for binary crates, not library crates.

The single most important build configuration choice is simple but [easy to overlook](https://users.rust-lang.org/t/why-my-rust-program-is-so-slow/47764/5): make sure you are using a [release build](https://doc.rust-lang.org/cargo/reference/profiles.html#release) rather than a [dev build](https://doc.rust-lang.org/cargo/reference/profiles.html#dev) when you want high performance. This is usually done by specifying the `--release` flag to Cargo.

Dev builds are the default. They are good for debugging, but are not optimized. They are produced if you run `cargo build` or `cargo run`. (Alternatively, running `rustc` without additional options also produces an unoptimized build.)

Consider the following final line of output from a `cargo build` run.

```rust
Finished dev [unoptimized + debuginfo] target(s) in 29.80s
```

This output indicates that a dev build has been produced. The compiled code will be placed in the `target/debug/` directory. `cargo run` will run the dev build.

In comparison, release builds are much more optimized, omit debug assertions and integer overflow checks, and omit debug info. 10-100x speedups over dev builds are common! They are produced if you run `cargo build --release` or `cargo run --release`. (Alternatively, `rustc` has multiple options for optimized builds, such as `-O` and `-C opt-level`.) This will typically take longer than a dev build because of the additional optimizations.

Consider the following final line of output from a `cargo build --release` run.

```rust
Finished release [optimized] target(s) in 1m 01s
```

This output indicates that a release build has been produced. The compiled code will be placed in the `target/release/` directory. `cargo run --release` will run the release build.

See the [Cargo profile documentation](https://doc.rust-lang.org/cargo/reference/profiles.html) for more details about the differences between dev builds (which use the `dev` profile) and release builds (which use the `release` profile).

The default build configuration choices used in release builds provide a good balance between the abovementioned characteristics such as compile times, runtime speed, and binary size. But there are many possible adjustments, as the following sections explain.

The following build configuration options are designed primarily to maximize runtime speed. Some of them may also reduce binary size.

The Rust compiler splits crates into multiple [codegen units](https://doc.rust-lang.org/cargo/reference/profiles.html#codegen-units) to parallelize (and thus speed up) compilation. However, this might cause it to miss some potential optimizations. You may be able to improve runtime speed and reduce binary size, at the cost of increased compile times, by setting the number of units to one. Add these lines to the `Cargo.toml` file:

```toml
[profile.release]
codegen-units = 1
```

[**Example 1**](http://likebike.com/posts/How_To_Write_Fast_Rust_Code.html#emit-asm),[**Example 2**](https://github.com/rust-lang/rust/pull/115554#issuecomment-1742192440).

[Link-time optimization](https://doc.rust-lang.org/cargo/reference/profiles.html#lto) (LTO) is a whole-program optimization technique that can improve runtime speed by 10-20% or more, and also reduce binary size, at the cost of worse compile times. It comes in several forms.

The first form of LTO is *thin local LTO*, a lightweight form of LTO. By default the compiler uses this for any build that involves a non-zero level of optimization. This includes release builds. To explicitly request this level of LTO, put these lines in the `Cargo.toml` file:

```toml
[profile.release]
lto = false
```

The second form of LTO is *thin LTO*, which is a little more aggressive, and likely to improve runtime speed and reduce binary size while also increasing compile times. Use `lto = "thin"` in `Cargo.toml` to enable it.

The third form of LTO is *fat LTO*, which is even more aggressive, and may improve performance and reduce binary size further (but [not always](https://github.com/rust-lang/rust/pull/103453)) while increasing build times again. Use `lto = "fat"` in `Cargo.toml` to enable it.

Finally, it is possible to fully disable LTO, which will likely worsen runtime speed and increase binary size but reduce compile times. Use `lto = "off"` in `Cargo.toml` for this. Note that this is different to the `lto = false` option, which, as mentioned above, leaves thin local LTO enabled.

It is possible to replace the default (system) heap allocator used by a Rust program with an alternative allocator. The exact effect will depend on the individual program and the alternative allocator chosen, but large improvements in runtime speed and large reductions in memory usage have been seen in practice. The effect will also vary across platforms, because each platform’s system allocator has its own strengths and weaknesses. The use of an alternative allocator is also likely to increase binary size and compile times.

One popular alternative allocator for Linux and Mac is [jemalloc](https://github.com/jemalloc/jemalloc), usable via the [`tikv-jemallocator`](https://crates.io/crates/tikv-jemallocator) crate. To use it, add a dependency to your `Cargo.toml` file:

```toml
[dependencies]
tikv-jemallocator = "0.5"
```

Then add the following to your Rust code, e.g. at the top of `src/main.rs`:

```rust
#[global_allocator]
static GLOBAL: tikv_jemallocator::Jemalloc = tikv_jemallocator::Jemalloc;
```

Furthermore, on Linux, jemalloc can be configured to use [transparent huge pages](https://www.kernel.org/doc/html/next/admin-guide/mm/transhuge.html) (THP). This can further speed up programs, possibly at the cost of higher memory usage.

Do this by setting the `MALLOC_CONF` environment variable (or perhaps [`_RJEM_MALLOC_CONF`](https://github.com/tikv/jemallocator/issues/65)) appropriately before building your program, for example:

```bash
MALLOC_CONF="thp:always,metadata_thp:always" cargo build --release
```

The system running the compiled program also has to be configured to support THP. See [this blog post](https://kobzol.github.io/rust/rustc/2023/10/21/make-rust-compiler-5percent-faster.html) for more details.

Another alternative allocator that works on many platforms is [mimalloc](https://github.com/microsoft/mimalloc), usable via the [`mimalloc`](https://crates.io/crates/mimalloc) crate. To use it, add a dependency to your `Cargo.toml` file:

```toml
[dependencies]
mimalloc = "0.1"
```

Then add the following to your Rust code, e.g. at the top of `src/main.rs`:

```rust
#[global_allocator]
static GLOBAL: mimalloc::MiMalloc = mimalloc::MiMalloc;
```

If you do not care about the compatibility of your binary on older (or other types of) processors, you can tell the compiler to generate the newest (and potentially fastest) instructions specific to a [certain CPU architecture](https://doc.rust-lang.org/rustc/codegen-options/index.html#target-cpu), such as AVX SIMD instructions for x86-64 CPUs.

To request these instructions from the command line, use the `-C target-cpu=native` flag. For example:

```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

Alternatively, to request these instructions from a [`config.toml`](https://doc.rust-lang.org/cargo/reference/config.html) file (for one or more projects), add these lines:

```toml
[build]
rustflags = ["-C", "target-cpu=native"]
```

This can improve runtime speed, especially if the compiler finds vectorization opportunities in your code.

If you are unsure whether `-C target-cpu=native` is working optimally, compare the output of `rustc --print cfg` and `rustc --print cfg -C target-cpu=native` to see if the CPU features are being detected correctly in the latter case. If not, you can use `-C target-feature` to target specific features.

Profile-guided optimization (PGO) is a compilation model where you compile your program, run it on sample data while collecting profiling data, and then use that profiling data to guide a second compilation of the program. This can improve runtime speed by 10% or more.[**Example 1**](https://blog.rust-lang.org/inside-rust/2020/11/11/exploring-pgo-for-the-rust-compiler.html),[**Example 2**](https://github.com/rust-lang/rust/pull/96978).

It is an advanced technique that takes some effort to set up, but is worthwhile in some cases. See the [rustc PGO documentation](https://doc.rust-lang.org/rustc/profile-guided-optimization.html) for details. Also, the [`cargo-pgo`](https://github.com/Kobzol/cargo-pgo) command makes it easier to use PGO (and [BOLT](https://github.com/llvm/llvm-project/tree/main/bolt), which is similar) to optimize Rust binaries.

Unfortunately, PGO is not supported for binaries hosted on crates.io and distributed via `cargo install`, which limits its usability.

The following build configuration options are designed primarily to minimize binary size. Their effects on runtime speed vary.

You can request an [optimization level](https://doc.rust-lang.org/cargo/reference/profiles.html#opt-level) that aims to minimize binary size by adding these lines to the `Cargo.toml` file:

```toml
[profile.release]
opt-level = "z"
```

This may also reduce runtime speed.

An alternative is `opt-level = "s"`, which targets minimal binary size a little less aggressively. Compared to `opt-level = "z"`, it allows [slightly more inlining](https://doc.rust-lang.org/rustc/codegen-options/index.html#inline-threshold) and also the vectorization of loops.

If you do not need to unwind on panic, e.g. because your program doesn’t use [`catch_unwind`](https://doc.rust-lang.org/std/panic/fn.catch_unwind.html), you can tell the compiler to simply [abort on panic](https://doc.rust-lang.org/cargo/reference/profiles.html#panic). On panic, your program will still produce a backtrace.

This might reduce binary size and increase runtime speed slightly, and may even reduce compile times slightly. Add these lines to the `Cargo.toml` file:

```toml
[profile.release]
panic = "abort"
```

You can tell the compiler to [strip](https://doc.rust-lang.org/cargo/reference/profiles.html#strip) symbols from a release build by adding these lines to `Cargo.toml`:

```toml
[profile.release]
strip = "symbols"
```

[**Example**](https://github.com/nnethercote/counts/commit/53cab44cd09ff1aa80de70a6dbe1893ff8a41142).

However, stripping symbols may make your compiled program more difficult to debug and profile. For example, if a stripped program panics, the backtrace produced may contain less useful information than normal. The exact effects depend on the platform.

Debug info does not need to be stripped from release builds. By default, debug info is not generated for local release builds, and debug info for the standard library has been stripped automatically in release builds [since Rust 1.77](https://blog.rust-lang.org/2024/03/21/Rust-1.77.0.html#enable-strip-in-release-profiles-by-default).

For more advanced binary size minimization techniques, consult the comprehensive documentation in the excellent [`min-sized-rust`](https://github.com/johnthagen/min-sized-rust) repository.

The following build configuration options are designed primarily to minimize compile times.

A big part of compile time is actually linking time, particularly when rebuilding a program after a small change. On some platforms it is possible to select a faster linker than the default one.

One option is [lld](https://lld.llvm.org/), which is available on Linux and Windows. lld has been the default linker on Linux [since Rust 1.90](https://blog.rust-lang.org/2025/09/01/rust-lld-on-1.90.0-stable/). It is not yet the default on Windows, but it should work for most use cases.

To specify lld from the command line, use the `-C link-arg=-fuse-ld=lld` flag. For example:

```bash
RUSTFLAGS="-C link-arg=-fuse-ld=lld" cargo build --release
```

Alternatively, to specify lld from a [`config.toml`](https://doc.rust-lang.org/cargo/reference/config.html) file (for one or more projects), add these lines:

```toml
[build]
rustflags = ["-C", "link-arg=-fuse-ld=lld"]
```

There is a [GitHub Issue](https://github.com/rust-lang/rust/issues/39915#issuecomment-618726211) tracking full support for lld.

Another option is [mold](https://github.com/rui314/mold), which is currently available on Linux. Simply substitute `mold` for `lld` in the instructions above. mold is often faster than lld.[**Example**](https://davidlattimore.github.io/posts/2024/02/04/speeding-up-the-rust-edit-build-run-cycle.html). It is also much newer and may not work in all cases.

A final option is [wild](https://github.com/davidlattimore/wild), which is currently only available on Linux. It may be even faster than mold, but it is less mature.

On Mac, an alternative linker isn’t necessary because the system linker is fast.

Unlike the other options in this chapter, there are no trade-offs to choosing another linker. As long as the linker works correctly for your program, which is likely to be true unless you are doing unusual things, an alternative linker can be dramatically faster without any downsides.

Although release builds give the best performance, many people use dev builds while developing because they build more quickly. If you use dev builds but don’t often use a debugger, consider disabling debuginfo. This can improve dev build times significantly, by as much as 20-40%.[**Example.**](https://kobzol.github.io/rust/rustc/2025/05/20/disable-debuginfo-to-improve-rust-compile-times.html)

To disable debug info generation, add these lines to the `Cargo.toml` file:

```toml
[profile.dev]
debug = false
```

Note that this means that stack traces will not contain line information. If you want to keep that line information, but do not require full information for the debugger, you can use `debug = "line-tables-only"` instead, which still gives most of the compile time benefits.

If you use nightly Rust, you can enable the experimental [parallel front-end](https://blog.rust-lang.org/2023/11/09/parallel-rustc.html). It may reduce compile times at the cost of higher compile-time memory usage. It won’t affect the quality of the generated code.

You can do that by adding `-Zthreads=N` to RUSTFLAGS, for example:

```bash
RUSTFLAGS="-Zthreads=8" cargo build --release
```

Alternatively, to enable the parallel front-end from a [`config.toml`](https://doc.rust-lang.org/cargo/reference/config.html) file (for one or more projects), add these lines:

```toml
[build]
rustflags = ["-Z", "threads=8"]
```

Values other than `8` are possible, but that is the number that tends to give the best results.

In the best cases, the experimental parallel front-end reduces compile times by up to 50%. But the effects vary widely and depend on the characteristics of the code and its build configuration, and for some programs there is no compile time improvement.

If you use nightly Rust you can enable the Cranelift codegen back-end on [some platforms](https://github.com/rust-lang/rustc_codegen_cranelift#platform-support). It may reduce compile times at the cost of lower quality generated code, and therefore is recommended for dev builds rather than release builds.

First, install the back-end with this `rustup` command:

```bash
rustup component add rustc-codegen-cranelift-preview --toolchain nightly
```

To select Cranelift from the command line, use the `-Zcodegen-backend=cranelift` flag. For example:

```bash
RUSTFLAGS="-Zcodegen-backend=cranelift" cargo +nightly build
```

Alternatively, to specify Cranelift from a [`config.toml`](https://doc.rust-lang.org/cargo/reference/config.html) file (for one or more projects), add these lines:

```toml
[unstable]
codegen-backend = true

[profile.dev]
codegen-backend = "cranelift"
```

For more information, see the [Cranelift documentation](https://github.com/rust-lang/rustc_codegen_cranelift).

In addition to the `dev` and `release` profiles, Cargo supports [custom profiles](https://doc.rust-lang.org/cargo/reference/profiles.html#custom-profiles). It might be useful, for example, to create a custom profile halfway between `dev` and `release` if you find the runtime speed of dev builds insufficient and the compile times of release builds too slow for everyday development.

There are many choices to be made when it comes to build configurations. The following points summarize the above information into some recommendations.

- If you want to maximize runtime speed, consider all of the following:`codegen-units = 1`, `lto = "fat"`, an alternative allocator, and `panic = "abort"`.
- If you want to minimize binary size, consider `opt-level = "z"`,`codegen-units = 1`, `lto = "fat"`, `panic = "abort"`, and `strip = "symbols"`.
- In either case, consider `-C target-cpu=native` if broad architecture support is not needed, and `cargo-pgo` if it works with your distribution mechanism.
- Always use a faster linker if you are on a platform that supports it, because there are no downsides to doing so.
- Use `cargo-wizard` if you need additional help with these choices.
- Benchmark all changes, one at a time, to ensure they have the expected effects.

Finally, [this issue](https://github.com/rust-lang/rust/issues/103595) tracks the evolution of the Rust compiler’s own build configuration. The Rust compiler’s build system is stranger and more complex than that of most Rust programs. Nonetheless, this issue may be instructive in showing how build configuration choices can be applied to a large program.
