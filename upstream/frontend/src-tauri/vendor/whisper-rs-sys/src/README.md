# Pre-generated whisper-rs-sys bindings

This directory contains `bindings.rs`, the file that `whisper-rs-sys`'s
`build.rs` would otherwise produce via bindgen at build time.

The project pins `WHISPER_DONT_GENERATE_BINDINGS=1` so the build uses
this vendored copy instead of invoking bindgen, which avoids the
LLVM/Clang toolchain dependency for the common case.

## Why a restore script is required

`bindings.rs` normally lives in the cargo registry cache at
`$CARGO_HOME/registry/src/index.crates.io-1949cf8c6b5b557f/whisper-rs-sys-0.11.1/src/`.
That location is treated as transient: a `cargo clean`, a toolchain
switch, or any operation that invalidates the cache removes the file.
Once it's gone, the build fails unless bindgen is available.

`scripts/restore-whisper-bindings.mjs` copies the vendored file back
into the cargo registry cache. Run it before building if the file
disappears:

```bash
node scripts/restore-whisper-bindings.mjs
```

## Long-term fix (not yet done)

The robust solution is to ship a small `vendor/whisper-rs-sys` crate
that contains only the binding file and use `[patch.crates-io]` to
redirect the dependency. That requires vendoring a few sibling files
(lib.rs, build.rs) which is invasive enough to be worth its own PR.
