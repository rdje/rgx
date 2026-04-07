# rgx CLI User Guide

The `rgx-cli` is a command-line interface for the rgx high-performance regex engine.
It supports pattern matching against inline text, files, and entire directory trees,
with output options ranging from simple spans to structured JSON.

---

## Table of Contents

1. [Quick Start](#quick-start)
2. [Basic Usage](#basic-usage)
3. [File Matching](#file-matching)
4. [Directory Scanning](#directory-scanning)
5. [Context Lines](#context-lines)
6. [Output Formats](#output-formats)
7. [Find and Replace](#find-and-replace)
8. [Filtering with Invert-Match](#filtering-with-invert-match)
9. [Code Blocks and Variables](#code-blocks-and-variables)
10. [Verbosity and Debugging](#verbosity-and-debugging)
11. [Complete Option Reference](#complete-option-reference)
12. [Examples Cookbook](#examples-cookbook)

---

## Quick Start

Match a pattern against inline text:

```bash
rgx-cli "hello" "hello world"
```

Search a file for all lines containing "ERROR":

```bash
rgx-cli --file app.log --line-mode "ERROR"
```

Find and replace across a codebase, printing results to stdout:

```bash
rgx-cli --file src/ --recursive --replace "new_name" "old_name"
```

---

## Basic Usage

The simplest invocation takes a pattern and input text as positional arguments:

```bash
rgx-cli PATTERN TEXT
```

This prints the byte-offset spans of each match:

```bash
rgx-cli "cat" "the cat sat on a cat mat"
# 4..7
# 18..21
```

If `TEXT` is omitted, rgx reads from standard input:

```bash
echo "abc 123 def" | rgx-cli "[0-9]+"
# 4..7
```

### Counting matches

Use `--count` to print only the total number of matches:

```bash
rgx-cli --count "the" "the cat sat on the mat"
# 2
```

---

## File Matching

### Whole-file mode (default)

Point `--file` at a file to match against its entire contents. Byte-offset spans
are reported relative to the start of the file:

```bash
rgx-cli --file data.txt "pattern"
```

### Line mode

Add `--line-mode` to match each line independently. Output is formatted as
`LINE_NUM: matched_text`:

```bash
rgx-cli --file app.log --line-mode "ERROR|WARN"
# 14: ERROR
# 37: WARN
```

### Counting file matches

Combine `--file` and `--count`:

```bash
rgx-cli --file app.log --count "ERROR"
# 42

rgx-cli --file app.log --line-mode --count "ERROR"
# 42
```

---

## Directory Scanning

### Recursive search (`--recursive` / `-r`)

When `--file` points to a directory, use `--recursive` (or `-r`) to scan every
file in the tree. Hidden directories (those starting with `.`) are automatically
skipped. Binary files are detected and excluded.

Output is formatted as `RELATIVE_PATH:LINE_NUM: matched_text`:

```bash
rgx-cli --file src/ --recursive "TODO|FIXME"
# src/main.rs:12: TODO
# src/lib.rs:45: FIXME
# src/utils/helpers.rs:8: TODO
```

With `--only-matching`:

```bash
rgx-cli --file . -r -o "TODO|FIXME|HACK"
# src/main.rs:12:TODO
# tests/integration.rs:3:FIXME
```

Recursive mode works with all other flags: `--count`, `--json`, `--replace`,
`--only-matching`, `--invert-match`, and `--context`.

```bash
rgx-cli --file src/ -r --count "unwrap"
# 37
```

---

## Context Lines

### `--context N` / `-C N`

Show N lines before and after each matching line, similar to `grep -C`. Groups
of matches separated by more than N lines are divided by `--` separators.

Matching lines are marked with `:`, context lines with `-`:

```bash
rgx-cli --file app.log --line-mode -C 2 "ERROR"
# 10-request received
# 11-processing payload
# 12:ERROR: null pointer exception
# 13-stack trace follows
# 14-  at main.rs:42
# --
# 50-retrying connection
# 51-timeout reached
# 52:ERROR: connection refused
# 53-falling back to cache
# 54-cache hit
```

Context works with recursive scanning too:

```bash
rgx-cli --file src/ -r -C 1 "panic"
```

---

## Output Formats

### JSON output (`--json`)

Produce machine-readable JSON, suitable for piping to `jq` or other processors:

```bash
rgx-cli --json "[0-9]+" "call 555-1234"
# [{"start":5,"end":8,"text":"555"},{"start":9,"end":13,"text":"1234"}]
```

In line-mode or recursive mode, entries include `line` and `file` fields:

```bash
rgx-cli --file app.log --line-mode --json "ERROR"
# [{"start":0,"end":5,"text":"ERROR","line":12}]

rgx-cli --file src/ -r --json "TODO"
# [{"start":7,"end":11,"text":"TODO","line":3,"file":"src/main.rs"}]
```

### Only-matching output (`--only-matching` / `-o`)

Print just the matched text, one match per line:

```bash
rgx-cli -o "[0-9]+" "abc 123 def 456"
# 123
# 456
```

In file or recursive mode:

```bash
rgx-cli --file app.log --line-mode -o "ERROR|WARN|INFO"
# ERROR
# WARN
# INFO
```

---

## Find and Replace

### `--replace STRING`

Replace every match with the given string and print the result to stdout.
The original file is never modified.

Inline text:

```bash
rgx-cli --replace "dog" "cat|kitten" "I have a cat and a kitten"
# I have a dog and a dog
```

File replacement (prints to stdout):

```bash
rgx-cli --file data.txt --replace "REDACTED" "[0-9]{3}-[0-9]{2}-[0-9]{4}"
```

Recursive replacement (only files with matches produce output):

```bash
rgx-cli --file src/ -r --replace "new_api" "old_api"
```

To actually update files in place, redirect or pipe through your own tooling:

```bash
rgx-cli --file config.yaml --replace "production" "staging" > config.yaml.new
mv config.yaml.new config.yaml
```

---

## Filtering with Invert-Match

### `--invert-match` / `-v`

Print lines that do NOT match the pattern. This is the complement of a normal
search, useful for filtering out noise:

```bash
rgx-cli --file app.log --invert-match "DEBUG"
# 1:INFO: application started
# 4:ERROR: connection failed
# 7:WARN: retrying
```

Combine with `--context` to see surrounding lines around non-matching lines:

```bash
rgx-cli --file app.log -v -C 1 "DEBUG"
```

Works with recursive scanning:

```bash
rgx-cli --file src/ -r -v "test"
```

And with inline text:

```bash
echo -e "keep\ndrop\nkeep" | rgx-cli -v "drop" ""
```

---

## Code Blocks and Variables

### Execution modes (`--mode`)

The rgx engine supports embedded code blocks in patterns. The `--mode` flag
controls which execution backends are available:

| Mode   | Description                                          |
|--------|------------------------------------------------------|
| `pure` | Maximum performance, regex matching only (default)   |
| `safe` | Code execution in sandboxed environments (Lua, WASM) |
| `full` | Enables native callbacks in addition to sandboxed    |

```bash
rgx-cli --mode safe '(?{lua:return 1})hello' "hello"
```

### Host variables (`--var`)

Pass key-value pairs to code blocks:

```bash
rgx-cli --mode full --var "env=prod" '(?{native:check_env})' ""
```

Multiple variables can be specified by repeating `--var`:

```bash
rgx-cli --mode full --var "threshold=100" --var "env=staging" 'pattern' "text"
```

### WASM modules (`--wasm-module`)

Register WebAssembly modules for `(?{wasm:module:function})` patterns:

```bash
rgx-cli --mode safe --wasm-module "validator=/path/to/validator.wasm" \
    '(?{wasm:validator:check})data' "data"
```

### Show details (`--show-details`)

Include branch numbers and code-block results in match output:

```bash
rgx-cli --show-details --mode full 'cat|dog' "a dog"
# 2..5 branch=2
```

---

## Verbosity and Debugging

### Verbosity levels (`--verbosity`)

Control the amount of diagnostic output:

| Level    | Description                    |
|----------|--------------------------------|
| `none`   | No diagnostic output (default) |
| `low`    | Basic operational messages      |
| `medium` | Intermediate detail            |
| `high`   | Detailed compile/execute logs  |
| `debug`  | Exhaustive trace-level logging |

```bash
rgx-cli --verbosity high "pattern" "text"
```

### Legacy shortcuts

- `--debug` / `-d`: equivalent to `--verbosity high`
- `--trace` / `-t`: equivalent to `--verbosity debug`
- `--quiet`: suppress all diagnostic output

### Trace log file (`--trace-log`)

Route diagnostic output to `trace.log` instead of the terminal:

```bash
rgx-cli --trace --trace-log "complex|pattern" "test input"
# (terminal shows only match results; trace.log has the diagnostics)
```

---

## Complete Option Reference

| Option                       | Short | Description                                                     |
|------------------------------|-------|-----------------------------------------------------------------|
| `--mode <MODE>`              |       | Execution mode: `pure`, `safe`, or `full` (default: `pure`)    |
| `--var <NAME=VALUE>`         |       | Set a host-provided code-block variable (repeatable)            |
| `--wasm-module <NAME=PATH>`  |       | Register a WASM module (repeatable)                             |
| `--file <PATH>`              |       | Read input from a file or directory                             |
| `--line-mode`                |       | Match each line independently (requires `--file`)               |
| `--recursive`                | `-r`  | Scan directories recursively (requires `--file`)                |
| `--count`                    |       | Print only the match count                                      |
| `--context <N>`              | `-C`  | Show N context lines around matches                             |
| `--replace <STRING>`         |       | Replace matches with STRING, print result                       |
| `--json`                     |       | Output matches as JSON                                          |
| `--only-matching`            | `-o`  | Print only the matched text                                     |
| `--invert-match`             | `-v`  | Print non-matching lines                                        |
| `--show-details`             |       | Include branch/code-block details in output                     |
| `--debug`                    | `-d`  | Enable high-verbosity output                                    |
| `--trace`                    | `-t`  | Enable debug-verbosity output                                   |
| `--verbosity <LEVEL>`        |       | Set verbosity: `none`, `low`, `medium`, `high`, `debug`         |
| `--quiet`                    |       | Suppress all diagnostic output                                  |
| `--trace-log`                |       | Write diagnostics to `trace.log` instead of terminal            |
| `--version`                  |       | Print version information                                       |
| `--help`                     |       | Print help information                                          |

---

## Examples Cookbook

### 1. Search log files for errors

```bash
rgx-cli --file /var/log/app.log --line-mode "ERROR|FATAL|CRITICAL"
```

### 2. Search with context to understand error surroundings

```bash
rgx-cli --file /var/log/app.log --line-mode -C 3 "FATAL"
```

### 3. Find TODOs across a codebase

```bash
rgx-cli --file src/ -r "TODO|FIXME|HACK|XXX"
```

### 4. Count TODOs per file (combine with shell tools)

```bash
for f in $(find src -name '*.rs'); do
    count=$(rgx-cli --file "$f" --line-mode --count "TODO|FIXME")
    [ "$count" -gt 0 ] && echo "$f: $count"
done
```

### 5. Extract all email addresses from text

```bash
rgx-cli -o "[a-zA-Z0-9._%+-]+@[a-zA-Z0-9.-]+\.[a-zA-Z]{2,}" "Contact us at info@example.com or support@test.org"
# info@example.com
# support@test.org
```

### 6. Extract URLs from a web page

```bash
curl -s https://example.com | rgx-cli -o "https?://[a-zA-Z0-9./?=_-]+" ""
```

### 7. Extract IP addresses from logs

```bash
rgx-cli --file access.log --line-mode -o "[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}\.[0-9]{1,3}"
```

### 8. Redact Social Security Numbers

```bash
rgx-cli --file customer_data.txt --replace "XXX-XX-XXXX" "[0-9]{3}-[0-9]{2}-[0-9]{4}"
```

### 9. Redact credit card numbers

```bash
rgx-cli --file transactions.csv --replace "****-****-****-****" "[0-9]{4}-[0-9]{4}-[0-9]{4}-[0-9]{4}"
```

### 10. JSON output for pipeline processing

```bash
rgx-cli --json "[0-9]+" "order 42 has 3 items" | jq '.[].text'
# "42"
# "3"
```

### 11. Recursive JSON output piped to jq

```bash
rgx-cli --file src/ -r --json "unwrap" | jq '.[] | "\(.file):\(.line)"'
```

### 12. Filter out DEBUG lines from logs

```bash
rgx-cli --file app.log --invert-match "DEBUG"
```

### 13. Filter out blank lines

```bash
rgx-cli --file messy.txt --invert-match "^$"
```

### 14. Find function definitions in Rust files

```bash
rgx-cli --file src/ -r -o "fn [a-zA-Z_][a-zA-Z0-9_]*"
```

### 15. Replace deprecated API calls

```bash
rgx-cli --file src/ -r --replace "new_database_connect" "old_db_connect"
```

### 16. Count matches across an entire project

```bash
rgx-cli --file . -r --count "unsafe"
```

### 17. Find lines with multiple matches using JSON

```bash
rgx-cli --file data.csv --line-mode --json "[0-9]+" | jq 'group_by(.line) | .[] | select(length > 1)'
```

### 18. Validate that a pattern exists in source

```bash
if rgx-cli --count "version = " "$(cat Cargo.toml)" | grep -q '^0$'; then
    echo "No version field found!"
    exit 1
fi
```

### 19. Extract key-value pairs

```bash
rgx-cli -o "[A-Z_]+=[a-zA-Z0-9_]+" "CONFIG_MODE=release DB_HOST=localhost"
# CONFIG_MODE=release
# DB_HOST=localhost
```

### 20. Search with WASM-powered validation

```bash
rgx-cli --mode safe \
    --wasm-module "check=/path/to/checker.wasm" \
    '(?{wasm:check:validate})[0-9]+' \
    "test 12345"
```
