[DON'T DELETE UNTIL FEEL COMPLETE] review duplicated and redundent logic from whole code base and remove and cleanup AND DRY.

continue with your recommendation, proceed with outcome. don't stop. review overall progress and changes again carefully, can you do better? go on don't ask me

--


---

---

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

Rust vectors contain three words: a length, a capacity, and a pointer. If you have a vector that is unlikely to be changed in the future, you can convert it to a _boxed slice_ with [`Vec::into_boxed_slice`](https://doc.rust-lang.org/std/vec/struct.Vec.html#method.into_boxed_slice). A boxed slice contains only two words, a length and a pointer. Any excess element capacity is dropped, which may cause a reallocation.

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

If you have many short vectors _and_ you precisely know their maximum length,`ArrayVec` from the [`arrayvec`](https://crates.io/crates/arrayvec) crate is a better choice than `SmallVec`. It does not require the fallback to heap allocation, which makes it a little faster.[**Example**](https://github.com/rust-lang/rust/pull/74310/commits/c492ca40a288d8a85353ba112c4d38fe87ef453e).

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

To ensure the number and/or size of allocations done by your code doesn’t increase unintentionally, you can use the _heap usage testing_ feature of [dhat-rs](https://crates.io/crates/dhat) to write tests that check particular code snippets allocate the expected amount of heap memory.

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

Cachegrind is a good profiler for determining if a function is inlined. When looking at Cachegrind’s output, you can tell that a function has been inlined if (and only if) its first and last lines are _not_ marked with event counts. For example:

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

The inverse of inlining is _outlining_: moving rarely executed code into a separate function. You can add a `#[cold]` attribute to such functions to tell the compiler that the function is rarely called. This can result in better code generation for the hot path.[**Example 1**](https://github.com/Lokathor/tinyvec/pull/127),[**Example 2**](https://crates.io/crates/fast_assert).

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

The first form of LTO is _thin local LTO_, a lightweight form of LTO. By default the compiler uses this for any build that involves a non-zero level of optimization. This includes release builds. To explicitly request this level of LTO, put these lines in the `Cargo.toml` file:

```toml
[profile.release]
lto = false
```

The second form of LTO is _thin LTO_, which is a little more aggressive, and likely to improve runtime speed and reduce binary size while also increasing compile times. Use `lto = "thin"` in `Cargo.toml` to enable it.

The third form of LTO is _fat LTO_, which is even more aggressive, and may improve performance and reduce binary size further (but [not always](https://github.com/rust-lang/rust/pull/103453)) while increasing build times again. Use `lto = "fat"` in `Cargo.toml` to enable it.

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

---

This article is about building actors with Tokio directly, without using any actor libraries such as Actix. This turns out to be rather easy to do, however there are some details you should be aware of:

1. Where to put the `tokio::spawn` call.
2. Struct with `run` method vs bare function.
3. Handles to the actor.
4. Backpressure and bounded channels.
5. Graceful shutdown.

The techniques outlined in this article should work with any executor, but for simplicity we will only talk about Tokio. There is some overlap with the [spawning](https://tokio.rs/tokio/tutorial/spawning) and [channel chapters](https://tokio.rs/tokio/tutorial/channels) from the Tokio tutorial, and I recommend also reading those chapters.

Before we can talk about how to write an actor, we need to know what an actor is. The basic idea behind an actor is to spawn a self-contained task that performs some job independently of other parts of the program. Typically these actors communicate with the rest of the program through the use of message passing channels. Since each actor runs independently, programs designed using them are naturally parallel.

A common use-case of actors is to assign the actor exclusive ownership of some resource you want to share, and then let other tasks access this resource indirectly by talking to the actor. For example, if you are implementing a chat server, you may spawn a task for each connection, and a master task that routes chat messages between the other tasks. This is useful because the master task can avoid having to deal with network IO, and the connection tasks can focus exclusively on dealing with network IO.

This article is also available as [a talk on YouTube](https://www.youtube.com/watch?v=fTXuGRP1ee4).

## The Recipe

An actor is split into two parts: the task and the handle. The task is the independently spawned Tokio task that actually performs the duties of the actor, and the handle is a struct that allows you to communicate with the task.

Let's consider a simple actor. The actor internally stores a counter that is used to obtain some sort of unique id. The basic structure of the actor would be something like the following:

```rust
use tokio::sync::{oneshot, mpsc};

struct MyActor {
    receiver: mpsc::Receiver<ActorMessage>,
    next_id: u32,
}
enum ActorMessage {
    GetUniqueId {
        respond_to: oneshot::Sender<u32>,
    },
}

impl MyActor {
    fn new(receiver: mpsc::Receiver<ActorMessage>) -> Self {
        MyActor {
            receiver,
            next_id: 0,
        }
    }
    fn handle_message(&mut self, msg: ActorMessage) {
        match msg {
            ActorMessage::GetUniqueId { respond_to } => {
                self.next_id += 1;

                // The \`let _ =\` ignores any errors when sending.
                //
                // This can happen if the \`select!\` macro is used
                // to cancel waiting for the response.
                let _ = respond_to.send(self.next_id);
            },
        }
    }
}

async fn run_my_actor(mut actor: MyActor) {
    while let Some(msg) = actor.receiver.recv().await {
        actor.handle_message(msg);
    }
}
```

Now that we have the actor itself, we also need a handle to the actor. A handle is an object that other pieces of code can use to talk to the actor, and is also what keeps the actor alive.

The handle will look like this:

```rust
#[derive(Clone)]
pub struct MyActorHandle {
    sender: mpsc::Sender<ActorMessage>,
}

impl MyActorHandle {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let actor = MyActor::new(receiver);
        tokio::spawn(run_my_actor(actor));

        Self { sender }
    }

    pub async fn get_unique_id(&self) -> u32 {
        let (send, recv) = oneshot::channel();
        let msg = ActorMessage::GetUniqueId {
            respond_to: send,
        };

        // Ignore send errors. If this send fails, so does the
        // recv.await below. There's no reason to check for the
        // same failure twice.
        let _ = self.sender.send(msg).await;
        recv.await.expect("Actor task has been killed")
    }
}
```

[full example](https://play.rust-lang.org/?version=stable&mode=debug&edition=2018&gist=1e60fb476843fb130db9034e8ead210c)

Let's take a closer look at the different pieces in this example.

**`ActorMessage.`** The `ActorMessage` enum defines the kind of messages we can send to the actor. By using an enum, we can have many different message types, and each message type can have its own set of arguments. We return a value to the sender by using an [`oneshot`](https://docs.rs/tokio/1/tokio/sync/oneshot/index.html) channel, which is a message passing channel that allows sending exactly one message.

In the example above, we match on the enum inside a `handle_message` method on the actor struct, but that isn't the only way to structure this. One could also match on the enum in the `run_my_actor` function. Each branch in this match could then call various methods such as `get_unique_id` on the actor object.

**Errors when sending messages.** When dealing with channels, not all errors are fatal. Because of this, the example sometimes uses `let _ =` to ignore errors. Generally a `send` operation on a channel fails if the receiver has been dropped.

The first instance of this in our example is the line in the actor where we respond to the message we were sent. This can happen if the receiver is no longer interested in the result of the operation, e.g. if the task that sent the message might have been killed.

**Shutdown of actor.** We can detect when the actor should shut down by looking at failures to receive messages. In our example, this happens in the following while loop:

```rust
while let Some(msg) = actor.receiver.recv().await {
    actor.handle_message(msg);
}
```

When all senders to the `receiver` have been dropped, we know that we will never receive another message and can therefore shut down the actor. When this happens, the call to `.recv()` returns `None`, and since it does not match the pattern `Some(msg)`, the while loop exits and the function returns.

**`#[derive(Clone)]`** The `MyActorHandle` struct derives the `Clone` trait. It can do this because [`mpsc`](https://docs.rs/tokio/1/tokio/sync/mpsc/index.html) means that it is a multiple-producer, single-consumer channel. Since the channel allows multiple producers, we can freely clone our handle to the actor, allowing us to talk to it from multiple places.

## A run method on a struct

The example I gave above uses a top-level function that isn't defined on any struct as the thing we spawn as a Tokio task, however many people find it more natural to define a `run` method directly on the `MyActor` struct and spawn that. This certainly works too, but the reason I give an example that uses a top-level function is that it more naturally leads you towards the approach that doesn't give you lots of lifetime issues.

To understand why, I have prepared an example of what people unfamiliar with the pattern often come up with.

```rust
impl MyActor {
    fn run(&mut self) {
        tokio::spawn(async move {
            while let Some(msg) = self.receiver.recv().await {
                self.handle_message(msg);
            }
        });
    }

    pub async fn get_unique_id(&self) -> u32 {
        let (send, recv) = oneshot::channel();
        let msg = ActorMessage::GetUniqueId {
            respond_to: send,
        };

        // Ignore send errors. If this send fails, so does the
        // recv.await below. There's no reason to check for the
        // same failure twice.
        let _ = self.sender.send(msg).await;
        recv.await.expect("Actor task has been killed")
    }
}

... and no separate MyActorHandle
```

The two sources of trouble in this example are:

1. The `tokio::spawn` call is inside `run`.
2. The actor and the handle are the same struct.

The first issue causes problems because the `tokio::spawn` function requires the argument to be `'static`. This means that the new task must own everything inside it, which is a problem because the method borrows `self`, meaning that it is not able to give away ownership of `self` to the new task.

The second issue causes problems because Rust enforces the single-ownership principle. If you combine both the actor and the handle into a single struct, you are (at least from the compiler's perspective) giving every handle access to the fields owned by the actor's task. E.g. the `next_id` integer should be owned only by the actor's task, and should not be directly accessible from any of the handles.

That said, there is a version that works. By fixing the two above problems, you end up with the following:

```rust
impl MyActor {
    async fn run(&mut self) {
        while let Some(msg) = self.receiver.recv().await {
            self.handle_message(msg);
        }
    }
}

impl MyActorHandle {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel(8);
        let actor = MyActor::new(receiver);
        tokio::spawn(async move { actor.run().await });

        Self { sender }
    }
}
```

This works identically to the top-level function. Note that, strictly speaking, it is possible to write a version where the `tokio::spawn` is inside `run`, but I don't recommend that approach.

## Variations on the theme

The actor I used as an example in this article uses the request-response paradigm for the messages, but you don't have to do it this way. In this section I will give some inspiration to how you can change the idea.

### No responses to messages

The example I used to introduce the concept includes a response to the messages sent over a `oneshot` channel, but you don't always need a response at all. In these cases there's nothing wrong with just not including the `oneshot` channel in the message enum. When there's space in the channel, this will even allow you to return from sending before the message has been processed.

You should still make sure to use a bounded channel so that the number of messages waiting in the channel don't grow without bound. In some cases this will mean that sending still needs to be an async function to handle the cases where the `send` operation needs to wait for more space in the channel.

However there is an alternative to making `send` an async method. You can use the `try_send` method, and handle sending failures by simply killing the actor. This can be useful in cases where the actor is managing a `TcpStream`, forwarding any messages you send into the connection. In this case, if writing to the `TcpStream` can't keep up, you might want to just close the connection.

### Multiple handle structs for one actor

If an actor needs to be sent messages from different places, you can use multiple handle structs to enforce that some message can only be sent from some places.

When doing this you can still reuse the same `mpsc` channel internally, with an enum that has all the possible message types in it. If you _do_ want to use separate channels for this purpose, the actor can use [`tokio::select!`](https://docs.rs/tokio/1/tokio/macro.select.html) to receive from multiple channels at once.

```rs
loop {
    tokio::select! {
        Some(msg) = chan1.recv() => {
            // handle msg
        },
        Some(msg) = chan2.recv() => {
            // handle msg
        },
        else => break,
    }
}
```

You need to be careful with how you handle when the channels are closed, as their `recv` method immediately returns `None` in this case. Luckily the `tokio::select!` macro lets you handle this case by providing the pattern `Some(msg)`. If only one channel is closed, that branch is disabled and the other channel is still received from. When both are closed, the else branch runs and uses `break` to exit from the loop.

### Actors sending messages to other actors

There is nothing wrong with having actors send messages to other actors. To do this, you can simply give one actor the handle of some other actor.

You need to be a bit careful if your actors form a cycle, because by holding on to each other's handle structs, the last sender is never dropped, preventing shutdown. To handle this case, you can have one of the actors have two handle structs with separate `mpsc` channels, but with a `tokio::select!` that looks like this:

```rs
loop {
    tokio::select! {
        opt_msg = chan1.recv() => {
            let msg = match opt_msg {
                Some(msg) => msg,
                None => break,
            };
            // handle msg
        },
        Some(msg) = chan2.recv() => {
            // handle msg
        },
    }
}
```

The above loop will always exit if `chan1` is closed, even if `chan2` is still open. If `chan2` is the channel that is part of the actor cycle, this breaks the cycle and lets the actors shut down.

An alternative is to simply call [`abort`](https://docs.rs/tokio/1/tokio/task/struct.JoinHandle.html#method.abort) on one of the actors in the cycle.

### Multiple actors sharing a handle

Just like you can have multiple handles per actor, you can also have multiple actors per handle. The most common example of this is when handling a connection such as a `TcpStream`, where you commonly spawn two tasks: one for reading and one for writing. When using this pattern, you make the reading and writing tasks as simple as you can — their only job is to do IO. The reader task will just send any messages it receives to some other task, typically another actor, and the writer task will just forward any messages it receives to the connection.

This pattern is very useful because it isolates the complexity associated with performing IO, meaning that the rest of the program can pretend that writing something to the connection happens instantly, although the actual writing happens sometime later when the actor processes the message.

## Beware of cycles

I already talked a bit about cycles under the heading “Actors sending messages to other actors”, where I discussed shutdown of actors that form a cycle. However, shutdown is not the only problem that cycles can cause, because a cycle can also result in a deadlock where each actor in the cycle is waiting for the next actor to receive a message, but that next actor wont receive that message until its next actor receives a message, and so on.

To avoid such a deadlock, you must make sure that there are no cycles of channels with bounded capacity. The reason for this is that the `send` method on a bounded channel does not return immediately. Channels whose `send` method always returns immediately do not count in this kind of cycle, as you cannot deadlock on such a `send`.

Note that this means that a oneshot channel cannot be part of a deadlocked cycle, since their `send` method always returns immediately. Note also that if you are using `try_send` rather than `send` to send the message, that also cannot be part of the deadlocked cycle.

Thanks to [matklad](https://matklad.github.io/) for pointing out the issues with cycles and deadlocks.

---

**Slow Rust Builds?**

Here are some tips to speed up your compile times. This list was originally released on my [private blog](https://endler.dev/), but I decided to update it and move it here.

All tips are roughly ordered by impact so you can start from the top and work your way down.

## Table of Contents

Make sure you use the latest Rust version:

```plain
rustup update
```

Making the Rust compiler faster is an [ongoing process](https://blog.mozilla.org/nnethercote/2020/04/24/how-to-speed-up-the-rust-compiler-in-2020/). Thanks to their hard work, compiler speed has improved [30-40% across the board year-to-date, with some projects seeing up to 45%+ improvements](https://www.reddit.com/r/rust/comments/cezxjn/compiler_speed_has_improved_3040_across_the_board/). It pays off to keep your toolchain up-to-date.

```shellscript
# Slow 🐢
cargo build

# Fast 🐇 (2x-3x speedup)
cargo check
```

Most of the time, you don’t even have to _compile_ your project at all; you just want to know if you messed up somewhere. Whenever you can, **skip compilation altogether**. What you need instead is laser-fast code linting, type- and borrow-checking.

Use `cargo check` instead of `cargo build` whenever possible. It will only check your code for errors, but not produce an executable binary.

Consider the differences in the number of instructions between `cargo check` on the left and `cargo debug` in the middle. (Pay attention to the different scales.)

![Speedup factors: check 1, debug 5, opt 20](https://corrode.dev/blog/tips-for-faster-rust-compile-times/cargo-check.png)

A sweet trick I use is to run it in the background with [`cargo watch`](https://github.com/passcod/cargo-watch). This way, it will `cargo check` whenever you change a file.

**Bonus**: Use `cargo watch -c` to clear the screen before every run.

```shellscript
# Install cargo-machete 🔪️
cargo install cargo-machete && cargo machete

# Install cargo-shear ✂️🐑
cargo install cargo-shear

# Install cargo-udeps 🧼🧹️
cargo install cargo-udeps --locked
```

Dependencies sometimes become obsolete after refactoring. From time to time it helps to check if you can remove any unused dependencies.

The above tools will list all unused dependencies in your project. Each tool has its limitations, producing both false positives and false negatives. Using all three tools together provides the best results.

```shellscript
Analyzing dependencies of crates in this directory...
cargo-machete found the following unused dependencies in <project>:
crate1 -- <project>/Cargo.toml:
        clap
crate2 -- <project>/crate2/Cargo.toml:
        anyhow
        async-once-cell
        dirs
        log
        tracing
        url
```

More info on the [cargo-machete](https://github.com/bnjbvr/cargo-machete), [cargo-shear](https://github.com/Boshen/cargo-shear), and [cargo-udeps](https://github.com/est31/cargo-udeps) project pages.

Thanks for mentioning `cargo-shear` and `cargo-udeps` to reader [Nicholas Nethercote](https://nnethercote.github.io/) who is the author of the [Rust Performance Book](https://nnethercote.github.io/perf-book/) and the famous [How to speed up the Rust compiler series](https://nnethercote.github.io/2025/05/22/how-to-speed-up-the-rust-compiler-in-may-2025.html).

1. Run `cargo update` to update to the latest [semver](https://semver.org/) compatible version.
2. Run [`cargo outdated -wR`](https://github.com/kbknapp/cargo-outdated) to find newer, possibly incompatible dependencies. Update those and fix code as needed.
3. Run `cargo tree --duplicate` to find dependencies which come in multiple versions. Aim to consolidate to a single version by updating dependencies that rely on older versions. (Thanks to /u/dbdr for [pointing this out](https://www.reddit.com/r/rust/comments/hdb5m4/tips_for_faster_rust_compile_times/fvm1r2w/).)

(Instructions by [/u/oherrala on Reddit](https://www.reddit.com/r/rust/comments/gi7v2v/is_it_wrong_of_me_to_think_that_rust_crates_have/fqe848y).)

On top of that, use [`cargo audit`](https://github.com/RustSec/cargo-audit) to get notified about any vulnerabilities which need to be addressed, or deprecated crates which need a replacement.

```shellscript
cargo build --timings
```

This gives information about how long each crate takes to compile.

![Diagram of cargo build –timings](https://corrode.dev/blog/tips-for-faster-rust-compile-times/cargo-concurrency-over-time.png)

The red line in this diagram shows the number of units (crates) that are currently waiting to be compiled (and are blocked by another crate). If there are a large number of crates bottlenecked on a single crate, focus your attention on improving that one crate to improve parallelism.

The meaning of the colors:

- _Waiting_ (red) — Crates waiting for a CPU slot to open.
- _Inactive_ (blue) — Crates that are waiting for their dependencies to finish.
- _Active_ (green) — Crates currently being compiled.

More info [in the documentation](https://doc.rust-lang.org/cargo/reference/timings.html).

If you like to dig deeper than `cargo --timings`, Rust compilation can be profiled with [`cargo rustc -- -Zself-profile`](https://blog.rust-lang.org/inside-rust/2020/02/25/intro-rustc-self-profile.html#profiling-the-compiler). The resulting trace file can be visualized with a flamegraph or the Chromium profiler:

![Image of Chrome profiler with all crates](https://corrode.dev/blog/tips-for-faster-rust-compile-times/chrome_profiler.png)

Another golden one is [`cargo-llvm-lines`](https://github.com/dtolnay/cargo-llvm-lines), which shows the number of lines generated and the number of copies of each generic function in the final binary. This can help you identify which functions are the most expensive to compile.

```plain
$ cargo llvm-lines | head -20

  Lines        Copies         Function name
  -----        ------         -------------
  30737 (100%)   1107 (100%)  (TOTAL)
   1395 (4.5%)     83 (7.5%)  core::ptr::drop_in_place
    760 (2.5%)      2 (0.2%)  alloc::slice::merge_sort
    734 (2.4%)      2 (0.2%)  alloc::raw_vec::RawVec<T,A>::reserve_internal
    666 (2.2%)      1 (0.1%)  cargo_llvm_lines::count_lines
    490 (1.6%)      1 (0.1%)  <std::process::Command as cargo_llvm_lines::PipeTo>::pipe_to
    476 (1.5%)      6 (0.5%)  core::result::Result<T,E>::map
    440 (1.4%)      1 (0.1%)  cargo_llvm_lines::read_llvm_ir
    422 (1.4%)      2 (0.2%)  alloc::slice::merge
    399 (1.3%)      4 (0.4%)  alloc::vec::Vec<T>::extend_desugared
    388 (1.3%)      2 (0.2%)  alloc::slice::insert_head
    366 (1.2%)      5 (0.5%)  core::option::Option<T>::map
    304 (1.0%)      6 (0.5%)  alloc::alloc::box_free
    296 (1.0%)      4 (0.4%)  core::result::Result<T,E>::map_err
    295 (1.0%)      1 (0.1%)  cargo_llvm_lines::wrap_args
    291 (0.9%)      1 (0.1%)  core::char::methods::<impl char>::encode_utf8
    286 (0.9%)      1 (0.1%)  cargo_llvm_lines::run_cargo_rustc
    284 (0.9%)      4 (0.4%)  core::option::Option<T>::ok_or_else
```

If your incremental builds are slower than expected, you might be bottlenecked on a particular crate in the rustc backend.

To diagnose this, you can use [`samply`](https://github.com/mstange/samply) combined with the `-Zhuman_readable_cgu_names=yes` flag to profile the compiler and identify which codegen units are taking the longest to compile:

```shellscript
samply record cargo build
```

This will help you identify if a particular crate is the long pole in your build. Once you’ve identified the bottleneck, you may be able to improve compile times by refactoring or splitting the problematic crate.

For more details on back-end parallelism in the Rust compiler, see [Nicholas Nethercote’s blog post](https://nnethercote.github.io/2023/07/11/back-end-parallelism-in-the-rust-compiler.html).

Another useful flag is `-Zprint-mono-items=yes`, which prints all [monomorphized items](https://rustc-dev-guide.rust-lang.org/backend/monomorph.html) during compilation. This can help you understand what’s being generated in each CGU.

```shellscript
RUSTFLAGS="-Zprint-mono-items=yes" cargo +nightly build                                               ⏎
```

_Thanks to [Caspar](https://slowrush.dev) (asparck on [Reddit](https://www.reddit.com/r/rust/comments/1q8x25s/debugging_a_slowcompiling_codegen_unit/)) for this tip!_

From time to time, it helps to shop around for more lightweight alternatives to popular crates.

Again, `cargo tree` is your friend here to help you understand which of your dependencies are quite _heavy_: they require many other crates, cause excessive network I/O and slow down your build. Then search for lighter alternatives.

Also, [`cargo-bloat`](https://github.com/RazrFalcon/cargo-bloat) has a `--time` flag that shows you the per-crate build time. Very handy!

Here are a few examples:

| Crate                                             | Alternative                                                                                          |
| ------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| [serde](https://github.com/bnjbvr/cargo-machete)  | [miniserde](https://github.com/dtolnay/miniserde), [nanoserde](https://github.com/not-fl3/nanoserde) |
| [reqwest](https://github.com/seanmonstar/reqwest) | [ureq](https://github.com/algesten/ureq)                                                             |
| [clap](https://github.com/clap-rs/clap)           | [lexopt](https://github.com/blyxxyz/lexopt)                                                          |

Here’s an example where switching crates reduced compile times [from 2:22min to 26 seconds](https://blog.kodewerx.org/2020/06/the-rust-compiler-isnt-slow-we-are.html).

Cargo has that neat feature called [workspaces](https://doc.rust-lang.org/book/ch14-03-cargo-workspaces.html), which allow you to split one big crate into multiple smaller ones. This code-splitting is great for avoiding repetitive compilation because only crates with changes have to be recompiled. Bigger projects like [servo](https://github.com/servo/servo/blob/master/Cargo.toml) and [vector](https://github.com/timberio/vector/blob/1629f7f82e459ae87f699e931ca2b89b9080cfde/Cargo.toml#L28-L34) make heavy use of workspaces to reduce compile times.

[`cargo-features-manager`](https://github.com/ToBinio/cargo-features-manager) is a relatively new tool that helps you to disable unused features of your dependencies.

```shellscript
cargo install cargo-features-manager
cargo features prune
```

From time to time, check the feature flags of your dependencies. A lot of library maintainers take the effort to split their crate into separate features that can be toggled off on demand. Maybe you don’t need all the default functionality from every crate?

For example, `tokio` has [a ton of features](https://github.com/tokio-rs/tokio/blob/2bc6bc14a82dc4c8d447521005e044028ae199fe/tokio/Cargo.toml#L26-L91) that you can disable if not needed.

Another example is `bindgen`, which enables `clap` support by default for its binary usage. This isn’t needed for library usage, which is the common use-case. Disabling that feature [improved compile time of rust-rocksdb by ~13s and ~9s for debug and release builds respectively](https://github.com/rust-rocksdb/rust-rocksdb/pull/491). Thanks to reader [Lilian Anatolie Moraru](https://github.com/lilianmoraru) for mentioning this.

It seems that switching off features doesn’t always improve compile time. (See [tikv’s experiences here](https://github.com/tikv/tikv/pull/4453#issuecomment-481789292).) It may still be a good idea for improving security by reducing the code’s attack surface. Furthermore, disabling features can help slim down the dependency tree.

You get a list of features of a crate when installing it with `cargo add`.

If you want to look up the feature flags of a crate, they are listed on [docs.rs](https://docs.rs/). E.g. check out [tokio’s feature flags](https://docs.rs/crate/tokio/latest/features).

After you removed unused features, check the diff of your `Cargo.lock` file to see all the unnecessary dependencies that got cleaned up.

```toml
[features]
# Basic feature for default functionality
default = []

# Optional feature for JSON support
json = ["serde_json"]

# Another optional feature for more expensive or complex code
complex_feature = ["some-expensive-crate"]
```

Not all the code in your project is equally expensive to compile. You can use Cargo features to split up your code into smaller chunks on a more granular level than crates. This way, you can compile only the functionality you need.

This is a common practice for libraries. For example, `serde` has a feature called `derive` that enables code generation for serialization and deserialization. It’s not always needed, so it’s disabled by default. Similarly, `Tokio` and `reqwest` have a lot of features that can be enabled or disabled.

You can do the same in your code. In the above example, the `json` feature in your `Cargo.toml` enables JSON support while the `complex_feature` feature enables another expensive code path.

Sometimes many crates rebuild even when no code has changed, often due to environment variable differences between build processes (like your Makefile builds vs rust-analyzer vs CI builds).

Use cargo’s fingerprint logging to identify exactly why rebuilds are triggered:

```shellscript
export CARGO_LOG="cargo::core::compiler::fingerprint=info"
export RUST_LOG=trace
cargo build -vv
```

Look for lines showing what caused the rebuild. For example:

```plain
INFO prepare_target: cargo::core::compiler::fingerprint: dirty: EnvVarChanged { name: "VIRTUAL_ENV", old_value: None, new_value: Some("/path/to/.venv") }

Dirty pyo3-build-config v0.24.1: the env variable VIRTUAL_ENV changed
```

See that “Dirty” line? It tells you exactly what caused the rebuild. You can grep for that line to find all rebuild causes.

Common culprits include:

- Environment variables (CC, CXX, VIRTUAL_ENV, PATH changes)
- Feature flag mismatches between different build tools
- Different cargo profiles being used
- Timestamp differences in generated files

Once you identify the cause, ensure consistency across all your build processes. For rust-analyzer in VS Code, you can configure matching environment variables in `.vscode/settings.json`:

```json
{
    "rust-analyzer.check.extraEnv": {
        "CC": "clang",
        "CXX": "clang++",
        "VIRTUAL_ENV": "/path/to/your/.venv"
    },
    "rust-analyzer.cargo.features": "all"
}
```

This debugging approach can dramatically reduce unnecessary rebuilds when different tools use inconsistent environments.

Credit: This technique was mentioned in [this write-up](https://github.com/twitu/twitu/blob/54a8915eac80562a15824988bac629583e6befd1/fixing-frequent-full-rust-builds-with-cargo-fingerprints.md) by [Ishan Bhanuka](https://github.com/twitu).

Another neat project is [sccache](https://github.com/mozilla/sccache) by Mozilla, which caches compiled crates to avoid repeated compilation.

I had this running on my laptop for a while, but the benefit was rather negligible, to be honest. It works best if you work on a lot of independent projects that share dependencies (in the same version). A common use-case is shared build servers.

Did you know that the Rust project is using an alternative compiler that runs in parallel with `rustc` for every CI build?

[rustc_codegen_cranelift](https://github.com/rust-lang/rustc_codegen_cranelift/), also called `CG_CLIF`, is an experimental backend for the Rust compiler that is based on the [Cranelift](https://cranelift.dev/) compiler framework.

Here is a comparison between `rustc` and Cranelift for some popular crates (blue means better):

![LLVM compile time comparison between rustc and cranelift in favor of cranelift](https://corrode.dev/blog/tips-for-faster-rust-compile-times/cranelift.png)

The compiler creates fully working executable binaries. They won’t be optimized as much, but they are great for local development.

A more detailed write-up is on [Jason Williams’ page](https://jason-williams.co.uk/a-possible-new-backend-for-rust), and the project code is [on Github](https://github.com/bjorn3/rustc_codegen_cranelift).

A [linker](<https://en.wikipedia.org/wiki/Linker_(computing)>) is a tool that combines multiple object files into a single executable.
It’s the last step in the compilation process.

You can check if your linker is a bottleneck by running:

```plain
cargo clean
cargo +nightly rustc --bin <your_binary_name> -- -Z time-passes
```

It will output the timings of each step, including link time:

```plain
...
time:   0.000   llvm_dump_timing_file
time:   0.001   serialize_work_products
time:   0.002   incr_comp_finalize_session_directory
time:   0.004   link_binary_check_files_are_writeable
time:   0.614   run_linker
time:   0.000   link_binary_remove_temps
time:   0.620   link_binary
time:   0.622   link_crate
time:   0.757   link
time:   3.836   total
    Finished dev [unoptimized + debuginfo] target(s) in 42.75s
```

If the `link` step is slow, you can try to switch to a faster alternative:

| Linker                                       | Platform    | Production Ready                                     | Description                                 |
| -------------------------------------------- | ----------- | ---------------------------------------------------- | ------------------------------------------- |
| [`lld`](https://lld.llvm.org/)               | Linux/macOS | Yes                                                  | Drop-in replacement for system linkers      |
| [`mold`](https://github.com/rui314/mold)     | Linux       | [Yes](https://news.ycombinator.com/item?id=29568454) | Optimized for Linux                         |
| [`zld`](https://github.com/michaeleisel/zld) | macOS       | No (deprecated)                                      | Drop-in replacement for Apple’s `ld` linker |

Rust 1.51 added a flag for faster incremental debug builds on macOS. It can make debug builds multiple seconds faster (depending on your use-case). Some engineers [report](https://jakedeichert.com/blog/reducing-rust-incremental-compilation-times-on-macos-by-70-percent/) that this flag alone reduces compilation times on macOS by **70%**.

Add this to your `Cargo.toml`:

```toml
[profile.dev]
split-debuginfo = "unpacked"
```

The flag might become the standard for macOS soon. It is already the [default on nightly](https://github.com/rust-lang/cargo/pull/9298).

**Gatekeeper** is a system on macOS, which runs security checks on binaries. This can cause Rust builds to be slower by a few seconds for each iteration. The solution is to add your terminal to the Developer Tools, which will cause processes run by it to be excluded from Gatekeeper.

1. Run `sudo spctl developer-mode enable-terminal` in your terminal.
2. Go to System Preferences, and then to Security & Privacy.
3. Under the Privacy tab, go to `Developer Tools`.
4. Make sure your terminal is listed and enabled. If you’re using any third-party terminals like iTerm or Ghostty, add them to the list as well.
5. Restart your terminal.

![Excluding the terminal from Gatekeeper inspectin in macOS Developer Tools](https://corrode.dev/blog/tips-for-faster-rust-compile-times/developer-tools.png)

Thanks to the [nextest](https://nexte.st/docs/installation/macos/) and [Zed](https://zed.dev/docs/development/macos#tips--tricks) developers for the tip.

Windows 11 includes [Dev Drive](https://learn.microsoft.com/en-us/windows/dev-drive/), a file system optimized for development. According to Microsoft, [you can expect a speed boost of around 20-30%](https://devblogs.microsoft.com/visualstudio/devdrive/) by using Dev Drive:

![Dev Drive Performance Chart](https://corrode.dev/blog/tips-for-faster-rust-compile-times/DevDrivePerfChart.png)

To improve Rust compilation speed, move these to a Dev Drive:

- Rust toolchain folder (`CARGO_HOME`)
- Your project code
- Cargo’s `target` directory

You can go one step further and **add the above folders to your antivirus exclusions as well** for another potential speedup. You can find exclusion settings in Windows Security under Virus & threat protection settings.

![Antivirus Exclusion Settings on Windows](https://corrode.dev/blog/tips-for-faster-rust-compile-times/windows-antivirus-exclusions.png)

Thanks to the [nextest team](https://nexte.st/docs/installation/windows/) for the tip.

Rust comes with a huge set of [settings for code generation](https://doc.rust-lang.org/rustc/codegen-options). It can help to look through the list and tweak the parameters for your project.

There are **many** gems in the [full list of codegen options](https://doc.rust-lang.org/rustc/codegen-options). For inspiration, here’s [bevy’s config for faster compilation](https://github.com/bevyengine/bevy/blob/3a2a68852c0a1298c0678a47adc59adebe259a6f/.cargo/config_fast_builds).

If you heavily use procedural macros in your project (e.g., if you use serde), it might be worth it to play around with opt-levels in your `Cargo.toml`.

```toml
[profile.dev.build-override]
opt-level = 3
```

As reader [jfmontanaro](https://github.com/jfmontanaro) mentioned on [Github](https://github.com/mre/endler.dev/issues/53):

> I think the reason it helps with build times is because it only applies to build scripts and proc-macros. Build scripts and proc-macros are unique because during a normal build, they are not only compiled but also executed (and in the case of proc-macros, they can be executed repeatedly). When your project uses a lot of proc-macros, optimizing the macros themselves can in theory save a lot of time.

Another approach is to try and sidestep the macro impact on compile times with [watt](https://github.com/dtolnay/watt), a tool that offloads macro compilation to Webassembly.

From the docs:

> By compiling macros ahead-of-time to Wasm, we save all downstream users of the macro from having to compile the macro logic or its dependencies themselves.
>
> Instead, what they compile is a small self-contained Wasm runtime (~3 seconds, shared by all macros) and a tiny proc macro shim for each macro crate to hand off Wasm bytecode into the Watt runtime (~0.3 seconds per proc-macro crate you depend on). This is much less than the 20+ seconds it can take to compile complex procedural macros and their dependencies.

Note that this crate is still experimental.

```shellscript
RUSTFLAGS="-Zmacro-stats" cargo +nightly build
```

Some macros have a big compile-time cost; but exactly how big? It can help to quantify the costs to see if a macro is worth optimizing (or removing). One way is to understand exactly how much code they generate.

One way is to use `cargo expand` to see the generated code, but that doesn’t scale to large codebases and the output is hard to quantify.

The alternative is to use the `-Zmacro-stats` flag to identify proc macros that generate a lot of code. This tool has already led to successful optimizations in projects like [Bevy](https://github.com/bevyengine/bevy/issues/19873) and [Arbitrary](https://nnethercote.github.io/2025/08/16/speed-wins-when-fuzzing-rust-code-with-derive-arbitrary.html).

For more information, read [Nicholas Nethercote’s blog post](https://nnethercote.github.io/2025/06/26/how-much-code-does-that-proc-macro-generate.html) on the topic.

Procedural macros need to parse Rust code, and that is a relatively complex task. Crates that depend on procedural macros will have to wait for the procedural macro to compile before they can compile. For example, `serde` can be a bottleneck in compilation times and can limit CPU utilization.

To improve Rust compile times, consider a strategic approach to handling serialization with Serde, especially in projects with a shared crate structure. Instead of placing Serde directly in a shared crate used across different parts of the project, you can make Serde an optional dependency through Cargo features.

Use the `cfg` or `cfg_attr` attributes to make Serde usage and `derive` in the shared crate feature-gated. This way, it becomes an optional dependency that is only enabled in leaf crates which actually perform serialization/deserialization.

This approach prevents the entire project from waiting on the compilation of Serde dependencies, which would be the case if Serde were a non-optional, direct dependency of the shared crate.

Let’s illustrate this with a simplified example. Imagine you have a Rust project with a shared library crate and a few other crates that depend on it. You don’t want to compile Serde unnecessarily when building parts of the project that don’t need it.

Here’s how you can structure your project to use optional features in Cargo:

In your `Cargo.toml` for the shared crate, declare serde as an optional dependency:

```toml
[package]
name = "shared"
version = "0.1.0"
edition = "2021"

[dependencies]
serde = { version = "1.0", optional = true }
```

In this crate, use conditional compilation to include serde only when the feature is enabled:

```rust
#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize};

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MySharedStruct {
    // Your struct fields
}
```

In the other crates, enable the `serde` feature for the shared crate if needed:

```toml
[package]
name = "other"
version = "0.1.0"
edition = "2021"

[dependencies]
shared = { path = "../shared", features = ["serde"] }
```

You can now use `MySharedStruct` with Serde’s functionality enabled without bloating the compilation of crates that don’t need it.

If you have a generic function, it will be compiled for every type you use it with. This can be a problem if you have a lot of different types.

A common solution is to use an inner non-generic function. This way, the compiler will only compile the inner function once.

This is a trick often used in the standard library. For example, here is the implementation of [`read_to_string`](https://github.com/rust-lang/rust/blob/ae612bedcbfc7098d1711eb35bc7ca994eb17a4c/library/std/src/fs.rs#L295-L304):

```rust
pub fn read_to_string<P: AsRef<Path>>(path: P) -> io::Result<String> {
    fn inner(path: &Path) -> io::Result<String> {
        let mut file = File::open(path)?;
        let size = file.metadata().map(|m| m.len() as usize).ok();
        let mut string = String::with_capacity(size.unwrap_or(0));
        io::default_read_to_string(&mut file, &mut string, size)?;
        Ok(string)
    }
    inner(path.as_ref())
}
```

You can do the same in your code: the outer function is generic, while it calls the inner non-generic function, which does the actual work.

Do you have a large Rust workspace with dependencies that:

1. Are used in multiple crates
2. Have different feature sets across those crates?

This situation can lead to long build times, as cargo will build each dependency multiple times with different features depending on which crate is being built. This is where [`cargo-hakari`](https://docs.rs/cargo-hakari/latest/cargo_hakari/about/index.html) comes in. It’s a tool designed to automatically manage “workspace-hack” crates.

In some scenarios, this can reduce consecutive build times by up to 50% or more. To learn more, check out the usage instructions and benchmarks in the [official cargo-hakari documentation](https://docs.rs/cargo-hakari/latest/cargo_hakari/about/index.html).

```shellscript
# Install the tool
cargo install cargo-add-dynamic

# Add a dynamic library to your project
cargo add-dynamic polars --features csv-file,lazy,list,describe,rows,fmt,strings,temporal
```

This will create a wrapper-crate around `polars` that is compiled as a dynamic library (`.so` on Linux, `.dylib` on macOS, `.dll` on Windows).

Essentially, it patches the dependency with

```toml
[lib]
crate-type = ["dylib"]
```

With this trick, you can save yourself the linking time of a dependency when you only change your own code. The dependency itself will only be recompiled when you change the features or the version. Of course, this works for any crate, not just `polars`.

Read more about this on [this blog post by Robert Krahn](https://robert.kra.hn/posts/2022-09-09-speeding-up-incremental-rust-compilation-with-dylibs/) and the [tool’s homepage](https://github.com/rksm/cargo-add-dynamic).

**In nightly**, you can now enable the new parallel compiler frontend. To try it out, run the nightly compiler with the `-Z threads=8` option:

```shellscript
RUSTFLAGS="-Z threads=8" cargo +nightly build
```

If you find that it works well for you, you can make it the default by adding `-Z threads=8` to your `~/.cargo/config.toml` file:

```toml
[build]
rustflags = ["-Z", "threads=8"]
```

Alternatively, you can set an alias for `cargo` in your shell’s config file (e.g., `~/.bashrc` or `~/.zshrc`):

```shellscript
alias cargo="RUSTFLAGS='-Z threads=8' cargo +nightly"
```

When the front-end is executed in a multi-threaded setting using `-Z threads=8`, benchmarks on actual code indicate that compilation times may decrease by as much as [50%](https://blog.rust-lang.org/2023/11/09/parallel-rustc.html). However, the gains fluctuate depending on the code being compiled. It is certainly worth a try, though.

Here is a visualization of the parallel compiler frontend in action:

![Result of the parallel compiler](https://corrode.dev/blog/tips-for-faster-rust-compile-times/samply-parallel.png)

Find out more on the official announcement [on the Rust blog](https://blog.rust-lang.org/2023/11/09/parallel-rustc.html).

Your filesystem might be the bottleneck. Consider using an in-memory filesystem like for your build directory.

Traditional temporary filesystem like `tmpfs` is limited to your RAM plus swap space and can be problematic for builds creating large intermediate artifacts.

Instead, on Linux, mount an `ext4` volume with the following options:

```plain
-o noauto_da_alloc,data=writeback,lazytime,journal_async_commit,commit=999,nobarrier
```

This will store files in the page cache if you have enough RAM, with writebacks occurring later. Treat this as if it were a temporary filesystem, as data may be lost or corrupted after a crash or power loss.

Credits go to [/u/The_8472 on Reddit](https://www.reddit.com/r/rust/comments/1ddgatd/compile_rust_faster_some_tricks/l85gzy8/).

If you reached this point, the easiest way to improve compile times even more is probably to spend money on top-of-the-line hardware.

As for laptops, the `M-series` of Apple’s new Macbooks perform really well for Rust compilation.

[![Rik Arends on Twitter](https://corrode.dev/blog/tips-for-faster-rust-compile-times/tweet.png)](https://twitter.com/rikarends/status/1328598935380910082)

The [benchmarks](https://www.reddit.com/r/rust/comments/qgi421/doing_m1_macbook_pro_m1_max_64gb_compile/) for a Macbook Pro with M1 Max are absolutely _ridiculous_ — even in comparison to the already fast M1:

| Project                                                   | M1 Max | M1 Air |
| --------------------------------------------------------- | ------ | ------ |
| [Deno](https://github.com/denoland)                       | 6m11s  | 11m15s |
| [MeiliSearch](https://github.com/meilisearch/MeiliSearch) | 1m28s  | 3m36s  |
| [bat](https://github.com/sharkdp/bat)                     | 43s    | 1m23s  |
| [hyperfine](https://github.com/sharkdp/hyperfine)         | 23s    | 42s    |
| [ripgrep](https://github.com/BurntSushi/ripgrep)          | 16s    | 37s    |

That’s a solid 2x performance improvement.

But if you rather like to stick to Linux, people also had great success with a multicore CPU like an [AMD Ryzen Threadripper and 32 GB of RAM](https://www.reddit.com/r/rust/comments/chqu4c/building_a_computer_for_fastest_possible_rust/).

On portable devices, compiling can drain your battery and be slow. To avoid that, I’m using my machine at home, a 6-core AMD FX 6300 with 12GB RAM, as a build machine. I can use it in combination with [Visual Studio Code Remote Development](https://code.visualstudio.com/docs/remote/remote-overview).

If you don’t have a dedicated machine yourself, you can offload the compilation process to the cloud instead.
[Gitpod.io](https://gitpod.io/) is superb for testing a cloud build as they provide you with a beefy machine (currently 16 core Intel Xeon 2.80GHz, 60GB RAM) for free during a limited period. Simply add `https://gitpod.io/#` in front of any Github URL.[Here is an example](https://gitpod.io/#https://github.com/hello-rust/show/tree/master/episode/9) for one of my [Hello Rust](https://corrode.dev/hello-rust/) episodes.

Gitpod has a neat feature called [prebuilds](https://www.gitpod.io/docs/prebuilds). From their docs:

> Whenever your code changes (e.g. when new commits are pushed to your repository), Gitpod can prebuild workspaces. Then, when you do create a new workspace on a branch, or Pull/Merge Request, for which a prebuild exists, this workspace will load much faster, because **all dependencies will have been already downloaded ahead of time, and your code will be already compiled**.

Especially when reviewing pull requests, this could give you a nice speedup. Prebuilds are quite customizable; take a look at the [`.gitpod.yml` config of nushell](https://github.com/nushell/nushell/blob/d744cf8437614cc6b95a4bb22731269a17fe9c80/.gitpod.yml) to get an idea.

If you have a slow internet connection, a big part of the initial build process is fetching all those shiny crates from crates.io. To mitigate that, you can download **all** crates in advance to have them cached locally.[criner](https://github.com/the-lean-crate/criner) does just that:

```plain
git clone https://github.com/the-lean-crate/criner
cd criner
cargo run --release -- mine
```

The archive size is surprisingly reasonable, with roughly **50GB of required disk space** (as of today).

```shellscript
cargo install cargo-nextest
cargo nextest run
```

It’s nice that `cargo` comes with its own little test runner, but especially if you have to build multiple test binaries, [`cargo nextest`](https://nexte.st/) can be up to 60% faster than `cargo test` thanks to its parallel execution model. Here are some quick [benchmarks](https://nexte.st/book/benchmarks.html):

| Project     | Revision   | Test count | cargo test (s) | nextest (s) | Improvement |
| ----------- | ---------- | ---------- | -------------- | ----------- | ----------- |
| crucible    | `cb228c2b` | 483        | 5.14           | 1.52        | 3.38×       |
| guppy       | `2cc51b41` | 271        | 6.42           | 2.80        | 2.29×       |
| mdBook      | `0079184c` | 199        | 3.85           | 1.66        | 2.31×       |
| meilisearch | `bfb1f927` | 721        | 57.04          | 28.99       | 1.96×       |
| omicron     | `e7949cd1` | 619        | 444.08         | 202.50      | 2.19×       |
| penumbra    | `4ecd94cc` | 144        | 125.38         | 90.96       | 1.37×       |
| reqwest     | `3459b894` | 113        | 5.57           | 2.26        | 2.48×       |
| ring        | `450ada28` | 179        | 13.12          | 9.40        | 1.39×       |
| tokio       | `1f50c571` | 1138       | 24.27          | 11.60       | 2.09×       |

Have any [integration tests](https://doc.rust-lang.org/rust-by-example/testing/integration_testing.html)? (These are the ones in your `tests` folder.) Did you know that the Rust compiler will create a binary for every single one of them? And every binary will have to be linked individually. This can take most of your build time because linking is slooow. 🐢 The reason is that many system linkers (like `ld`) are [single threaded](https://stackoverflow.com/questions/5142753/can-gcc-use-multiple-cores-when-linking).

To make the linker’s job a little easier, you can put all your tests in one crate. (Basically create a `main.rs` in your test folder and add your test files as `mod` in there.)

Then the linker will go ahead and build a single binary only. Sounds nice, but careful: it’s still a trade-off as you’ll need to expose your internal types and functions (i.e. make them `pub`).

If you have a lot of integration tests, this can [result in a 50% speedup](https://azriel.im/will/2019/10/08/dev-time-optimization-part-1-1.9x-speedup-65-less-disk-usage/).

_This tip was brought to you by [Luca Palmieri](https://twitter.com/algo_luca),[Lucio Franco](https://twitter.com/lucio_d_franco), and [Azriel Hoh](https://twitter.com/im_azriel). Thanks!_

```rust
#[test]
fn completion_works_with_real_standard_library() {
  if std::env::var("RUN_SLOW_TESTS").is_err() {
    return;
  }
  ...
}
```

If you have slow tests, you can put them behind an environment variable to disable them by default. This way, you can skip them locally and only run them on CI.

(A nice trick I learned from [matklad’s (Alex Kladov) post](https://matklad.github.io/2021/05/31/how-to-test.html).)

Many of the techniques in this article also apply to CI builds. For CI-specific optimizations and best practices, check out my dedicated guide on [Tips for Faster CI Builds](/blog/tips-for-faster-ci-builds/), which covers caching strategies, workflow optimization, and GitHub Actions-specific improvements.

For GitHub actions in particular you can also use [Swatinem/rust-cache](https://github.com/Swatinem/rust-cache).

It is as simple as adding a single step to your workflow:

```yaml
jobs:
    test:
        runs-on: ubuntu-latest
        steps:
            - uses: actions/checkout@v4
            - uses: dtolnay/rust-toolchain@stable
            - uses: Swatinem/rust-cache@v2
            - run: cargo test --all
```

With that, your dependencies will be cached between builds, and you can expect a significant speedup.

```yaml
- name: Compile
  run: cargo test --no-run --locked

- name: Test
  run: cargo test -- --nocapture --quiet
```

This makes it easier to find out how much time is spent on compilation and how much on running the tests.

```yaml
env:
    CARGO_INCREMENTAL: 0
```

Since CI builds are more akin to from-scratch builds, incremental compilation adds unnecessary dependency-tracking and IO overhead, reducing caching effectiveness.[Here’s how to disable it.](https://github.com/rust-analyzer/rust-analyzer/blob/25368d24308d6a94ffe8b99f0122bcf5a2175322/.github/workflows/ci.yaml#L11)

```toml
[profile.dev]
debug = 0
strip = "debuginfo"
```

Avoid linking debug info to speed up your build process, especially if you rarely use an actual debugger. There are two ways to avoid linking debug information: set `debug=0` to skip compiling it, or set `strip="debuginfo"` to skip linking it. Unfortunately, changing these options can trigger a full rebuild with Cargo.

- On Linux, set both for improved build times.
- On Mac, use `debug=0` since rustc uses an external strip command.
- On Windows, test both settings to see which is faster.

Note that without debug info, backtraces will only show function names, not line numbers. If needed, use `split-debuginfo="unpacked"` for a compromise.

As a nice side-effect, this will also help shrink the size of `./target`, improving caching efficiency.

Here is a [sample config](https://github.com/rust-analyzer/rust-analyzer/blob/48f84a7b60bcbd7ec5fa6434d92d9e7a8eb9731b/Cargo.toml#L6-L10) for how to apply the settings.

Avoid using `#![deny(warnings)]` in your code to prevent repetitive declarations. Furthermore, it is fine to get warnings during local development.

Instead, [add `-D warnings` to `RUSTFLAGS`](https://github.com/rust-analyzer/rust-analyzer/blob/3dae94bf2b3e496adb049da589c7efef272a39b8/.github/workflows/ci.yaml#L15) to globally deny warnings in all crates on CI.

```yaml
env:
    RUSTFLAGS: -D warnings
```

```diff
- runs-on: ubuntu-latest
+ runs-on: ubicloud
```

Services like [Ubicloud](https://www.ubicloud.com/use-cases/github-actions),[BuildJet](https://buildjet.com), or [RunsOn](https://github.com/runs-on/runs-on) provide you with faster workers for your Github Actions builds. Especially for Rust pipelines, the number of cores can have a significant big impact on compile times, so it might be worth a try.

Here is an example from the [Facebook Folly](https://github.com/facebook/folly) project using Ubicloud. Granted, this is a C++ project, but it shows the potential of faster runners:

![facebook/folly build times](https://corrode.dev/blog/tips-for-faster-rust-compile-times/ubicloud-facebook-folly.svg)

After signing up with the service, you only need to change the runner in your Github Actions workflow file.

Building Docker images from your Rust code? These can be notoriously slow, because cargo doesn’t support building only a project’s dependencies yet, invalidating the Docker cache with every build if you don’t pay attention.[`cargo-chef`](https://www.lpalmieri.com/posts/fast-rust-docker-builds/) to the rescue! ⚡

> `cargo-chef` can be used to fully leverage Docker layer caching, therefore massively speeding up Docker builds for Rust projects. On our commercial codebase (~14k lines of code, ~500 dependencies) we measured a **5x speed-up**: we cut Docker build times from **~10 minutes to ~2 minutes.**

Here is an example `Dockerfile` if you’re interested:

```docker
# Step 1: Compute a recipe file
FROM rust as planner
WORKDIR app
RUN cargo install cargo-chef
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

# Step 2: Cache project dependencies
FROM rust as cacher
WORKDIR app
RUN cargo install cargo-chef
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --recipe-path recipe.json

# Step 3: Build the binary
FROM rust as builder
WORKDIR app
COPY . .
# Copy over the cached dependencies from above
COPY --from=cacher /app/target target
COPY --from=cacher /usr/local/cargo /usr/local/cargo
RUN cargo build --release --bin app

# Step 4:
# Create a tiny output image.
# It only contains our final binary.
FROM rust as runtime
WORKDIR app
COPY --from=builder /app/target/release/app /usr/local/bin
ENTRYPOINT ["/usr/local/bin/app"]
```

[`cargo-chef`](https://github.com/LukeMathWalker/cargo-chef) can help speed up your continuous integration with Github Actions or your deployment process to Google Cloud.

Earthly is a relatively new build tool that is designed to be a replacement for Makefiles, Dockerfiles, and other build tools. It provides fast, incremental Rust builds for CI.

> Earthly speeds up Rust builds in CI by effectively implementing Cargo’s caching and Rust’s incremental compilation. This approach significantly reduces unnecessary rebuilds in CI, mirroring the efficiency of local Rust builds.
>
> Source: [Earthly for Rust](https://earthly.dev/rust)

They use a system called Satellites, which are persistent remote build runners that retain cache data locally. This can drastically speed up CI build times by eliminating cache uploads and downloads. Instead of bringing the cache data to the compute, they colocate the cache data and compute, eliminating cache transfers altogether. Less I/O means faster builds.

Earthly also provides a `lib/rust` library, which abstracts away cache configuration entirely. It ensures that Rust is caching correctly and building incrementally in CI. It can be used in your [`Earthfile`](https://docs.earthly.dev/docs/earthfile) like this:

```docker
IMPORT github.com/earthly/lib/rust
```

If you’re curious, [Earthly’s Guide for Rust](https://earthly.dev/rust) details a simple Rust example with optimized caching and compilation steps.

If you find that build times in your development environment are slow, here are a few additional tips you can try.

If you’re using Visual Studio Code and find that **debug sessions** are slow, make sure you don’t have too many breakpoints set. [Each breakpoint can slow down the debug session](https://www.reddit.com/r/rust/comments/1ddktag/looking_for_some_help_where_it_takes_a_minute_to/).

In case you have multiple projects open in Visual Studio Code, **each instance runs its own copy of rust-analyzer**. This can slow down your machine. Close unrelated projects if they aren’t needed.

If you’re using rust-analyzer in VS Code and find that you run into slow build times when saving your changes, it could be that the cache gets invalidated. This also results in dependencies like `serde` being rebuilt frequently.

You can fix this by configuring a separate target directory for rust-analyzer. Add this to your VS Code settings (preferably user settings):

```json
{
    "rust-analyzer.cargo.targetDir": true
}
```

This will make rust-analyzer build inside `target/rust-analyzer` instead of the default `target/` directory, preventing interference with your regular `cargo run` builds.

Some users reported [significant speedups](https://github.com/rust-lang/rust-analyzer/issues/6007#issuecomment-2563288106) thanks to that:

```plain
before: 34.98s user 2.02s system 122% cpu 30.176 total
after:   2.62s user 0.60s system 84% cpu 3.803 total
```

This could also help with [rust analyzer blocking debug builds](https://github.com/rust-lang/rust-analyzer/issues/4616).

Credit: This tip was shared by [asparck on Reddit](https://www.reddit.com/r/rust/comments/1if5wpm/high_hopes_for_rust_where_are_we/majhmhe/).

In this article, we’ve covered a lot of ground. We’ve looked at how to speed up your Rust builds by using better hardware, optimizing your code, and using better tools.

For CI-specific optimizations, don’t forget to check out [Tips for Faster CI Builds](/blog/tips-for-faster-ci-builds/), which complements the techniques discussed here.
