#!/usr/bin/env python3
"""
Extract hot paths from a samply / Firefox Profiler `profile.json.gz`.

samply records raw addresses; symbolication happens at `load` time
in the UI. For headless / CI-friendly analysis we re-symbolicate the
profile here using `atos` (macOS) or `addr2line` (Linux/macOS) over
the rgx binary on disk.

Reports two views per profile:

  1. Self-time top-N:    leaf-frame functions, ranked by sample count
                         (these are where wall-clock cycles are spent)
  2. Inclusive top-N:    any-frame functions, ranked by sample count
                         (these tell you which call paths dominate)

Filters out kernel / system frames (libsystem*, dyld, etc.) by default
so the rgx_core symbols stand out. Override with --no-system-filter.

Usage:
    scripts/samply-hotpaths.py target/samply-profiles/email_basic.find_first.json.gz
    scripts/samply-hotpaths.py target/samply-profiles/*.json.gz   # batch mode
"""

from __future__ import annotations

import argparse
import gzip
import json
import os
import shutil
import subprocess
import sys
from collections import Counter
from pathlib import Path


SYSTEM_PREFIXES = (
    "_dyld_",
    "_pthread_",
    "_libc_",
    "libsystem_",
    "libsystem_malloc.dylib::",
    "libsystem_kernel.dylib::",
    "libsystem_pthread.dylib::",
    "libsystem_platform.dylib::",
    "libsystem_c.dylib::",
    "libdyld.dylib::",
    "dyld::",
    "_os_",
    "__platform_",
    "__exit",
    "__sigtramp",
    "__commpage_",
    "DYLD-STUB",
)


def find_rgx_binary() -> Path | None:
    """Locate the perf_profile_targets binary built with the
    `profiling` cargo profile. Used as the symbolication source."""
    repo_root = Path(__file__).resolve().parent.parent
    candidate = repo_root / "target" / "profiling" / "examples" / "perf_profile_targets"
    if candidate.is_file():
        return candidate
    return None


def get_text_base(binary: Path) -> int:
    """Read the binary's __TEXT segment vmaddr (Mach-O on macOS) so we
    can offset samply's image-relative addresses correctly when
    calling `atos`. Defaults to 0x100000000 if otool is unavailable."""
    if sys.platform != "darwin":
        return 0
    if not shutil.which("otool"):
        return 0x100000000
    proc = subprocess.run(
        ["otool", "-l", str(binary)],
        capture_output=True,
        text=True,
        check=False,
    )
    in_text = False
    for line in proc.stdout.splitlines():
        s = line.strip()
        if s.startswith("segname __TEXT"):
            in_text = True
            continue
        if in_text and s.startswith("vmaddr "):
            return int(s.split()[1], 16)
    return 0x100000000


def symbolicate_addresses(
    binary: Path, addresses: list[int], text_base: int = 0
) -> dict[int, str]:
    """Resolve a batch of image-relative code addresses to function
    names using `atos` (macOS) or `addr2line` (Linux). Returns a map
    {original_address -> symbol_name_or_raw_hex}.

    samply records addresses normalised to the image base (0). atos
    expects file-address slides (Mach-O __TEXT vmaddr, typically
    0x100000000 on arm64). We add `text_base` before passing to atos
    and key the result on the original (unshifted) address so callers
    don't have to track the shift."""
    if not addresses:
        return {}

    if sys.platform == "darwin" and shutil.which("atos"):
        shifted = [a + text_base for a in addresses]
        addrs_hex = [hex(a) for a in shifted]
        result: dict[int, str] = {a: hex(a) for a in addresses}
        # atos accepts up to several thousand addresses per invocation;
        # chunk to be safe on argv length.
        for off in range(0, len(addrs_hex), 500):
            chunk_hex = addrs_hex[off : off + 500]
            chunk_origs = addresses[off : off + 500]
            proc = subprocess.run(
                ["atos", "-o", str(binary), "-l", hex(text_base)] + chunk_hex,
                capture_output=True,
                text=True,
                check=False,
            )
            lines = proc.stdout.splitlines()
            for orig, addr_hex, line in zip(chunk_origs, chunk_hex, lines):
                line = line.strip()
                if line and line != addr_hex and not line.startswith("0x"):
                    # atos format: "FuncName (in libname) (file:line)"
                    if " (in " in line:
                        line = line.split(" (in ")[0]
                    elif line.endswith(")"):
                        idx = line.rfind(" (")
                        if idx > 0:
                            line = line[:idx]
                    result[orig] = line
        return result

    if shutil.which("addr2line"):
        # On Linux, samply records addresses are typically file-relative
        # already; pass them as-is.
        addrs_hex = [hex(a) for a in addresses]
        result = {a: hex(a) for a in addresses}
        for off in range(0, len(addrs_hex), 500):
            chunk = addrs_hex[off : off + 500]
            proc = subprocess.run(
                ["addr2line", "-fipe", str(binary), "-a"] + chunk,
                capture_output=True,
                text=True,
                check=False,
            )
            cur_addr = None
            for line in proc.stdout.splitlines():
                line = line.strip()
                if line.startswith("0x"):
                    cur_addr = int(line, 16)
                elif cur_addr is not None:
                    name = line.split(" at ", 1)[0]
                    if name and name not in ("??", ""):
                        result[cur_addr] = name
                    cur_addr = None
        return result

    return {a: hex(a) for a in addresses}


def is_system_frame(name: str) -> bool:
    return name.startswith(SYSTEM_PREFIXES)


def shorten(name: str, max_len: int = 78) -> str:
    if len(name) <= max_len:
        return name
    # For Rust mangled paths, keep tail (function name) over head (module path).
    return "…" + name[-(max_len - 1):]


def analyse(path: Path, top_n: int = 15, system_filter: bool = True, binary: Path | None = None) -> None:
    open_fn = gzip.open if path.suffix == ".gz" else open
    with open_fn(path, "rt") as f:
        prof = json.load(f)

    main_threads = [t for t in prof.get("threads", []) if t.get("isMainThread")]
    if not main_threads:
        main_threads = prof.get("threads", [])
    if not main_threads:
        print(f"!! no threads in {path}", file=sys.stderr)
        return

    print(f"\n=== {path.name} ===")
    for thread in main_threads:
        strings = thread["stringArray"]
        funcs = thread["funcTable"]
        frames = thread["frameTable"]
        stacks = thread["stackTable"]
        samples = thread["samples"]

        func_names = [strings[n] if n is not None else "<unknown>" for n in funcs["name"]]
        # Re-symbolicate function names that look like raw addresses
        # (e.g. "0x4c9e6f") against the binary. Only resolve frames
        # whose `funcTable.resource` points at the rgx binary (we
        # don't have debuginfo for libsystem).
        if binary is not None:
            text_base = get_text_base(binary)
            res_table = thread["resourceTable"]
            res_lib = res_table.get("lib", [])
            res_name_idx = res_table.get("name", [])
            libs = prof.get("libs", [])
            binary_name = binary.name

            def is_rgx_resource(res_idx: int | None) -> bool:
                if res_idx is None or res_idx >= len(res_lib):
                    return False
                lib_idx = res_lib[res_idx]
                if lib_idx is None or lib_idx >= len(libs):
                    return False
                return libs[lib_idx].get("name", "") == binary_name

            raw_addr_indices: dict[int, list[int]] = {}
            funcs_resource = funcs.get("resource", [None] * len(func_names))
            for i, n in enumerate(func_names):
                if not n.startswith("0x"):
                    continue
                if is_rgx_resource(funcs_resource[i]):
                    try:
                        addr = int(n, 16)
                        raw_addr_indices.setdefault(addr, []).append(i)
                    except ValueError:
                        pass
                else:
                    # Non-rgx address — tag with the library name so
                    # libsystem_* / dyld frames are at least
                    # identifiable in the ranking even if we can't
                    # symbolicate them without dyld_shared_cache work.
                    res_idx = funcs_resource[i]
                    if res_idx is not None and res_idx < len(res_lib):
                        lib_idx = res_lib[res_idx]
                        if lib_idx is not None and lib_idx < len(libs):
                            lib_name = libs[lib_idx].get("name", "")
                            if lib_name and lib_name != binary_name:
                                func_names[i] = f"{lib_name}::{n}"
            if raw_addr_indices:
                resolved = symbolicate_addresses(
                    binary, list(raw_addr_indices.keys()), text_base=text_base
                )
                for addr, idxs in raw_addr_indices.items():
                    name = resolved.get(addr, hex(addr))
                    for i in idxs:
                        func_names[i] = name

        frame_to_func = frames["func"]
        # Each stack is a (parent, frame) pair forming a linked list to the leaf.
        stack_parent = stacks["prefix"]
        stack_frame = stacks["frame"]
        sample_stacks = samples["stack"]

        if not sample_stacks:
            print(f"  thread {thread.get('name')!r}: no samples")
            continue

        # Build per-stack frame chain (leaf-first).
        def chain_funcs(stack_idx: int) -> list[str]:
            chain: list[str] = []
            cur = stack_idx
            while cur is not None:
                frame_idx = stack_frame[cur]
                func_idx = frame_to_func[frame_idx]
                chain.append(func_names[func_idx])
                cur = stack_parent[cur]
            return chain

        self_counter: Counter[str] = Counter()
        incl_counter: Counter[str] = Counter()
        total = 0

        for stack_idx in sample_stacks:
            if stack_idx is None:
                continue
            chain = chain_funcs(stack_idx)
            if system_filter:
                chain = [c for c in chain if not is_system_frame(c)]
            if not chain:
                continue
            total += 1
            self_counter[chain[0]] += 1
            for fn in set(chain):
                incl_counter[fn] += 1

        if total == 0:
            print(f"  thread {thread.get('name')!r}: no rgx samples after filter")
            continue

        print(f"  thread {thread.get('name')!r}  total_samples={total}")
        print(f"\n  Self-time top-{top_n} (leaf-frame, where cycles ARE):")
        for fn, count in self_counter.most_common(top_n):
            pct = count / total * 100
            print(f"    {pct:5.1f}%  {count:>5}  {shorten(fn)}")

        print(f"\n  Inclusive-time top-{top_n} (any-frame, dominant call paths):")
        for fn, count in incl_counter.most_common(top_n):
            pct = count / total * 100
            print(f"    {pct:5.1f}%  {count:>5}  {shorten(fn)}")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("profiles", nargs="+", type=Path)
    ap.add_argument("--top", type=int, default=15)
    ap.add_argument(
        "--no-system-filter",
        action="store_true",
        help="Include libsystem_/dyld/etc. frames in the ranking.",
    )
    ap.add_argument(
        "--binary",
        type=Path,
        default=None,
        help="Binary to symbolicate raw addresses against. Defaults to "
        "target/profiling/examples/perf_profile_targets when found.",
    )
    args = ap.parse_args()

    binary = args.binary if args.binary else find_rgx_binary()
    if binary and not binary.is_file():
        print(f"!! binary not found, addresses will stay raw: {binary}", file=sys.stderr)
        binary = None

    for p in args.profiles:
        if not p.is_file():
            print(f"!! not a file: {p}", file=sys.stderr)
            continue
        analyse(p, top_n=args.top, system_filter=not args.no_system_filter, binary=binary)
    return 0


if __name__ == "__main__":
    sys.exit(main())
