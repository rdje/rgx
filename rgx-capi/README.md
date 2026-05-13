# rgx-capi

C ABI bindings for the rgx-core regex engine.

## What this is

`rgx-capi` is the FFI foundation that lets non-Rust programs use the
[rgx](https://github.com/rdje/rgx) engine through a stable C API.
It's the universal entry point — one cbindgen-generated header
(`include/rgx.h`) makes rgx callable from Go (cgo), Python (ctypes /
cffi), Julia (`ccall`), Zig (`@cImport`), Ruby (`fiddle`), PHP (FFI),
Swift, Kotlin/Native, and anywhere C interop is available.

## What this isn't

- It's **not** an idiomatic Python / Go / Julia / etc. wrapper. Per-
  language wrappers are separate projects layered on top of this C
  ABI. See `docs/A9_LANGUAGE_BINDINGS_DESIGN.md` §5 Phase 7 for the
  rollout plan.
- It's **not** the way Rust users should consume rgx. Rust callers
  should use `rgx-core` directly for the full Rust-native API
  (closures, `Cow<str>`, `Iterator` impls, ownership semantics).

## Status

**Phase 1 (scaffolding + basic matching)**, per
`docs/A9_LANGUAGE_BINDINGS_DESIGN.md` §5. Surface:

- Lifecycle: `rgx_compile`, `rgx_regex_free`, `rgx_regex_retain`.
- Diagnostics: `rgx_last_error`, `rgx_runtime_version_{major,minor,patch}`.
- Basic matching: `rgx_is_match`, `rgx_find_first`.

Phases 2 through 6 (captures, iterators, replace, safety limits,
embedded scripting hosts, observers, `tail_file`) ship in subsequent
commits.

## Build

```sh
cargo build -p rgx-capi --release
```

Produces:

- `target/release/librgx.{so,dylib,a,lib,rlib}` — the library
  artefacts (cdylib + staticlib + rlib).
- `rgx-capi/include/rgx.h` — auto-generated header (committed for
  inspection at the source tree).

## C-side usage

```c
#include "rgx.h"

int main(void) {
    RgxRegex* re = NULL;
    const char* pat = "\\d+";
    int32_t rc = rgx_compile((const uint8_t*)pat, 4, &re);
    if (rc != RGX_OK) {
        fprintf(stderr, "compile failed: %s\n", rgx_last_error());
        return 1;
    }

    const char* text = "abc 123 def";
    int32_t matched = 0;
    size_t start = 0, end = 0;
    rgx_find_first(re, (const uint8_t*)text, strlen(text),
                   &matched, &start, &end);

    if (matched) {
        printf("matched at %zu..%zu\n", start, end);
    }

    rgx_regex_free(re);
    return 0;
}
```

## ABI stability

Function signatures, error codes, and `#[repr(C)]` struct layouts are
stable within a major version. Opaque pointer internals are not —
never depend on the size or layout of any `Rgx*` type.

Use `rgx_runtime_version_*` to verify the loaded library matches the
header you compiled against.

## License

Apache-2.0. Same as the rest of the rgx workspace.
