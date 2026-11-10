# vtcode-threads

Thread identifiers extracted from `vtcode-core` for independent builds.

Owns `ThreadId` and `SubmissionId` (StringNewtype identifiers) previously defined in `vtcode_core::core::threads`. `vtcode-core` re-exports these via `pub use vtcode_threads::{ThreadId, SubmissionId}`.

## Usage

```rust
use vtcode_threads::{SubmissionId, ThreadId};

let id = ThreadId::new("thread-123");
let sub = SubmissionId::generate();
```
