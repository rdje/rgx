# Using rgx from Other Languages (C ABI)

rgx ships a stable C ABI in the `rgx-capi` workspace crate. One implementation effort — a cbindgen-generated `rgx.h` plus a `cdylib` / `staticlib` — makes rgx callable from any language with C FFI: Go (cgo), Python (ctypes / cffi), Julia (`ccall`), Zig (`@cImport`), Ruby (`fiddle`), PHP FFI, Swift, Kotlin/Native, and others.

This chapter documents the C surface and how to consume it. Idiomatic per-language wrappers (e.g., `pip install rgx`, `go get rgx`) are SEPARATE projects layered on top of this C ABI — see [Why rgx?](../why-rgx.md) for the broader value proposition and the design doc at `docs/A9_LANGUAGE_BINDINGS_DESIGN.md` for the full architecture.

## Status

| Phase | Surface | Status |
|-------|---------|--------|
| 1 | compile, free, retain, is_match, find_first, error, version | **shipped** |
| 2 | captures + iterators + replace | planned |
| 3 | configuration + safety limits + execution-mode introspection | planned |
| 4 | `tail_file` | planned |
| 5 | observers / structured events | planned |
| 6 | embedded scripting hosts (Lua / JS / Rhai / WASM / native) | planned |
| 7 | per-language idiomatic wrappers | on demand |

Phase 1 is enough to match patterns end-to-end and to validate the FFI boundary in real client code. Subsequent phases land additively.

## Building

```sh
cargo build -p rgx-capi --release
```

Produces, in `target/release/`:

- `librgx_capi.so` (Linux) / `librgx_capi.dylib` (macOS) — dynamic library.
- `librgx_capi.a` (Linux + macOS) — static library.
- `librgx_capi.rlib` — Rust-side rlib (you rarely want this; Rust callers should use `rgx-core` directly).

The C header is regenerated on every build and lives at `rgx-capi/include/rgx.h`. It is also committed to the source tree so contributors can inspect the API without running a build.

A CI gate (`scripts/check-capi-abi.sh`, per the [ABI stability contract](https://github.com/rdje/rgx/blob/main/rgx-capi/STABILITY.md)) enforces two invariants on every push: the committed header must be byte-identical to a fresh cbindgen regeneration (you cannot forget to commit it), and any ABI-meaningful change to the header must come with a workspace version bump. The header is therefore a trustworthy, always-current description of the ABI.

## The API surface (Phase 1)

```c
#include "rgx.h"
```

### Lifecycle

```c
int32_t rgx_compile(const uint8_t *pattern,
                    uintptr_t      pattern_len,
                    RgxRegex     **out_regex);

void         rgx_regex_free(RgxRegex *re);
RgxRegex*    rgx_regex_retain(RgxRegex *re);
```

- `rgx_compile` populates `*out_regex` with a fresh handle on success. Returns `RGX_OK` (0) on success or a negative error code.
- `rgx_regex_free` releases a handle. Passing `NULL` is a no-op; passing the same non-null pointer twice is undefined.
- `rgx_regex_retain` creates an additional handle to the same compiled regex (cheap `Arc::clone` under the hood). Both handles must be freed independently. Use this when handing a regex to a worker thread.

### Diagnostics

```c
const char* rgx_last_error(void);

uint32_t rgx_runtime_version_major(void);
uint32_t rgx_runtime_version_minor(void);
uint32_t rgx_runtime_version_patch(void);
```

- `rgx_last_error` returns the calling thread's most recent error message, or an empty string if no error has been recorded. The pointer is borrowed from per-thread storage — do NOT free it and do NOT use it after another `rgx_*` call on the same thread.
- The three `version_*` functions return the loaded library's `MAJOR.MINOR.PATCH`. Compare them at runtime to detect header/library mismatch.

### Matching

```c
int32_t rgx_is_match(const RgxRegex *re,
                     const uint8_t  *text,
                     uintptr_t       text_len,
                     int32_t        *out_matched);

int32_t rgx_find_first(const RgxRegex *re,
                       const uint8_t  *text,
                       uintptr_t       text_len,
                       int32_t        *out_matched,
                       uintptr_t      *out_start,
                       uintptr_t      *out_end);
```

- Both write `1` to `*out_matched` if a match exists, `0` otherwise. `rgx_find_first` additionally writes the match span (`*out_start`, `*out_end`) as byte offsets into `text`.

### Error codes

| Constant | Value | Meaning |
|----------|------:|---------|
| `RGX_OK`                  |    0 | Operation succeeded. |
| `RGX_ERR_NULL_POINTER`    |   -1 | A required pointer argument was null. |
| `RGX_ERR_INVALID_PATTERN` |   -2 | Pattern failed to compile. See `rgx_last_error`. |
| `RGX_ERR_INVALID_UTF8`    |   -3 | Reserved for input-UTF-8 violations. |
| `RGX_ERR_LIMIT_EXCEEDED`  |   -5 | Reserved for Phase 3 safety limits. |
| `RGX_ERR_INVALID_HANDLE`  |   -7 | A handle was passed to a function that doesn't accept it. |
| `RGX_ERR_INTERNAL`        |  -99 | Unexpected panic from the Rust engine. Please report. |

The numbering is deliberately sparse — gaps reserve room for future categories without renumbering existing codes. The enumeration is **append-only**: no code is ever removed or repurposed within a major version.

## End-to-end C example

```c
#include <stdint.h>
#include <stdio.h>
#include <string.h>

#include "rgx.h"

int main(void) {
    RgxRegex *re = NULL;
    const char *pat = "\\d+";
    if (rgx_compile((const uint8_t*)pat, strlen(pat), &re) != RGX_OK) {
        fprintf(stderr, "compile failed: %s\n", rgx_last_error());
        return 1;
    }

    const char *text = "abc 123 def";
    int32_t matched = 0;
    size_t start = 0, end = 0;
    rgx_find_first(re, (const uint8_t*)text, strlen(text),
                   &matched, &start, &end);

    if (matched) {
        printf("matched at %zu..%zu (\"%.*s\")\n",
               start, end, (int)(end - start), text + start);
    }

    rgx_regex_free(re);
    return 0;
}
```

Compile and link:

```sh
# Linux
cc demo.c -I rgx-capi/include target/release/librgx_capi.a \
    -lpthread -ldl -lm -o demo

# macOS
cc demo.c -I rgx-capi/include target/release/librgx_capi.a \
    -framework CoreFoundation -framework CoreServices \
    -framework Security -framework SystemConfiguration -o demo
```

Or link dynamically against `librgx_capi.{so,dylib}` — replace the staticlib path with `-L target/release -lrgx_capi`.

## Cross-FFI design principles

The C surface inherits four properties from the design doc that callers and per-language wrapper authors should understand:

1. **No panic crosses FFI.** Every public entry point wraps its body in `panic::catch_unwind`. A caught panic converts to `RGX_ERR_INTERNAL`; the panic message is retrievable via `rgx_last_error`. The library never aborts the calling process.
2. **Out-params, not return values, for results.** Every fallible function returns `int32_t` (0 = success, negative = error code). This pattern is friendlier to languages whose FFI tooling treats multi-value returns awkwardly (Go in particular, but also Python via ctypes).
3. **Explicit `(ptr, len)` byte buffers everywhere.** No null-terminated string assumption. Patterns and text inputs are raw bytes. This matters because rgx supports patterns and text that contain embedded NUL bytes (the `BytesRegex` path), and because some host languages count strings differently than C.
4. **Opaque pointers with explicit retain/release.** `RgxRegex` is a forward-declared `typedef struct RgxRegex RgxRegex;` in the header — the C side only ever holds the pointer. Internal layout is free to change between minor versions.

## ABI stability

- Function signatures, error codes, and `#[repr(C)]` struct layouts are **stable within a major version**.
- Opaque pointer internals are not — never depend on the size or layout of any `Rgx*` type.
- Error codes are **append-only**: new codes can be added; existing codes never change value, never get repurposed.
- `rgx_runtime_version_*` lets callers verify the loaded library matches the header version they compiled against. This guards against silent ABI drift when a library is upgraded out of sync with downstream consumers.

## Threading

- `RgxRegex*` is thread-safe for reading. Multiple threads may call `rgx_is_match` / `rgx_find_first` on the same handle concurrently — the underlying `Regex` is `Send + Sync`.
- `rgx_last_error` is per-thread. The pointer it returns is valid until the next `rgx_*` call on the same thread.
- The recommended pattern for handing a regex to a worker thread is: spawning thread calls `rgx_regex_retain` to mint an independent handle, hands the new handle to the worker, and each thread frees its handle independently. The shared `Arc<Regex>` underneath ensures the engine survives until the last handle is freed.

## What's missing in Phase 1

Phase 1 ships the smallest surface that lets clients match patterns end-to-end. The following are deliberately deferred:

- **Captures.** Phase 2 adds `rgx_captures_*` (allocate, extract by index, extract by name, free) plus iterator entry points (`rgx_find_iter_*`).
- **Replace.** Phase 2 adds `rgx_replace_first` / `rgx_replace_all` / `rgx_replace_with_callback`. The callback variant is the bridge to host-language-driven replacement.
- **Safety limits.** Phase 3 adds `rgx_set_max_steps`, `rgx_set_max_backtrack_frames`, `rgx_set_max_recursion_depth`, `rgx_set_max_trail_entries`. `RGX_ERR_LIMIT_EXCEEDED` is reserved for these.
- **Execution-mode introspection.** Phase 3 adds `rgx_uses_c2`, `rgx_uses_tdfa`, `rgx_uses_jit` — counterparts to the Rust-side `Regex::uses_c2()` / `uses_tdfa()` / `uses_jit()`.
- **`tail_file`.** Phase 4 adds the streaming entry point with a C function-pointer callback for each new match.
- **Observers / structured events.** Phase 5 surfaces the event-emit callbacks that drive rgx's introspection capability.
- **Embedded scripting hosts.** Phase 6 exposes `rgx_register_code_block_*` so callers can plug in Lua / JS / Rhai / WASM / native handlers from the host side.

See `docs/A9_LANGUAGE_BINDINGS_DESIGN.md` §5 for the full staging plan and §6 for the per-phase correctness gates.

## Building per-language wrappers

The C ABI is the **foundation**, not the destination. A Go consumer using cgo or a Python consumer using ctypes works today, but the resulting code is not idiomatic — Go users expect `regex.MatchString`, Python users expect `re.match`-style ergonomics, Julia users expect `Regex` to feel like a builtin.

Per-language wrappers are SEPARATE projects layered on top of `rgx-capi`. They live in their own repositories, ship through their own package managers, and version independently from the core engine. The design doc §5 Phase 7 lists the initial priority order, demand-driven: Go → Python → Julia → Zig → Ruby/PHP. Anyone is welcome to start a wrapper at any time — the C ABI is the contract.

## Related material

- [Why rgx?](../why-rgx.md) — what rgx adds beyond a regex library, and which of those features survive an FFI boundary.
- `docs/A9_LANGUAGE_BINDINGS_DESIGN.md` — the full design document covering all seven phases, threading guarantees, ABI stability mechanics, risk table, and open questions.
- `rgx-capi/README.md` — quick-start build and usage notes, kept in sync with this chapter.
