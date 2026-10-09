# VTCode image for Anthropic OSS Scanner.
# Built once WITH network, then audited WITHOUT network.
# Contract: checkout is at /src, deps pre-fetched, project built.
FROM rust:1.98.1-bookworm

ENV DEBIAN_FRONTEND=noninteractive \
    CARGO_INCREMENTAL=0 \
    CARGO_TERM_COLOR=never

RUN apt-get update && apt-get install -y --no-install-recommends \
      ca-certificates \
      build-essential \
      pkg-config \
      libssl-dev \
      clang \
      cmake \
      git \
      python3 \
    && rm -rf /var/lib/apt/lists/*

# scanner contract: the checkout lives inside the image at /src
COPY . /src
WORKDIR /src

# Pre-fetch all crates so offline audit can build/test without network.
RUN cargo fetch --locked

# Build as maintainers build it, with debug info (-g) and frame pointers.
# A step that fails here fails the build and notifies primary_contact.
RUN RUSTFLAGS="-g -fno-omit-frame-pointer" cargo build --locked --workspace -j"$(nproc)"

# Compile tests so agents can run them offline. Do not let failures abort the build.
RUN RUSTFLAGS="-g -fno-omit-frame-pointer" cargo test --locked --workspace --no-run -j"$(nproc)" || echo "WARNING: cargo test --no-run reported failures"

# Run a cheap subset to prove the image works. Leave full binaries/test drivers in place.
RUN (cargo test --locked -p vtcode-commons -j"$(nproc)" || echo "WARNING: cheap tests reported failures")
