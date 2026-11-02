# api-ffi-boundary

> Structure FFI: `foo-sys` for importing C, `foo-ffi` for exporting Rust

## Why It Matters

FFI is where Rust's safety guarantee ends - every cross-boundary call is the code's
responsibility because the compiler can't check the other side. A disciplined boundary
lays out ownership, allocation, and panics explicitly so the other side cannot corrupt
your invariants through data.

## Bad

```rust
// Mixed concerns: logic in the boundary crate, panic escapes into C,
// allocation freed on the wrong side.
#[no_mangle]
pub extern "C" fn process(raw: *const u8, len: usize) -> *mut i64 {
    let data = unsafe { std::slice::from_raw_parts(raw, len) };
    run_logic(data).expect("must work") // panics unwind into C = UB
}

fn run_logic(data: &[u8]) -> Option<i64> {
    let owned: *mut i64 = Box::into_raw(Box::new(compute(data)));
    owned // now C must know to free() it - with what allocator?
}
```

## Good

```rust
// Business logic lives in a safe core crate; only translation here.
// `repr(C)` is the layout contract with the other side.
#[repr(C)]
pub struct Deposit {
    account_id: u64,
    amount_cents: i64,
}

// Raw binding crate (foo-sys, or hand-written):
unsafe extern "C" {
    pub fn parse_header(buf: *const u8, len: usize) -> *mut RawHeader;
}

// Safe wrapper crate (foo): RAII wrapper frees on the same side.
pub struct Header(*mut RawHeader);

impl Header {
    pub fn parse(buf: &[u8]) -> Result<Self, ApiError> {
        if buf.is_empty() {
            return Err(ApiError::Empty);
        }
        let raw = unsafe { sys::parse_header(buf.as_ptr(), buf.len()) };
        if raw.is_null() {
            return Err(ApiError::Rejected);
        }
        Ok(Header(raw))
    }
}

impl Drop for Header {
    fn drop(&mut self) {
        // SAFETY: exactly one Box allocated in this process, freed here.
        unsafe {
            let _ = Box::from_raw(self.0 as *mut RawOwned);
        }
    }
}
```

## The Rules

| Rule | Detail |
|------|--------|
| Layout: `#[repr(C)]` | On every shared struct. Collect from both sides to catch drift if the layouts change. |
| Unified types | `u32`/`i64` etc., never `isize` / pointer-scaled types. |
| Strings | `CString` / `CStr`, not Rust `String`. Panics stopped at the boundary. |
| Same allocator | Allocate and free on the same side. Never mix `Rust Box` with `C free()` or vice versa. |
| Ownership | Wrap C-owned pointers in an RAII type whose `Drop` frees them; `Box::into_raw`/`from_raw` on export. |
| Errors | Check for null; return `Result` or `-1`/`Abort`. |
| Panics | `catch_unwind` at the FFI boundary - a panic crossing into C is UB (and under `panic = "abort"` unwinding never happens at all). |
| Generate | `bindgen` in a `foo-sys` crate; business logic stays in a safe `foo` core crate. |
| C++ | Use `cxx` instead of hand-written `extern "C++"`. |
| Naming | `foo-sys` imports C libraries; `foo-ffi` exports Rust as a C-style library. |

## See Also

- [type-repr-transparent](type-repr-transparent.md) - `#[repr(transparent)]` for FFI layout
- [anti-unsound-api](anti-unsound-api.md) - Safe functions must never cause UB through safe calls
- [own-drop-contract](own-drop-contract.md) - Rust's ownership contract, freely hoisted to FFI
- [err-panic-policy](err-panic-policy.md) - Panics never escape the boundary
