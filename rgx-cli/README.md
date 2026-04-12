# rgx-cli

[![crates.io](https://img.shields.io/crates/v/rgx-cli.svg)](https://crates.io/crates/rgx-cli)

Command-line interface for the [rgx regex engine](https://crates.io/crates/rgx-core). A grep-like tool with color output, live file tailing, embedded-code-block dispatch, and structured event streaming.

## Install

```bash
cargo install rgx-cli
```

## Quick examples

```bash
# Colorized match output (auto-detects terminals)
rgx-cli --color always '\d+' "answer is 42"

# Tail a log file and highlight ERROR / WARN lines in real time
rgx-cli --file app.log --follow 'ERROR|WARN'

# Recursive search with counts
rgx-cli --recursive --count 'TODO|FIXME' src/

# JSON output for pipelines
rgx-cli --json '(?P<year>\d{4})-(?P<month>\d{2})' notes.txt

# Embedded Lua predicate (needs --features lua at install time)
cargo install rgx-cli --features lua
rgx-cli --mode safe --var limit=100 '(\d+)(?{lua:return tonumber(arg[1]) <= vars.limit})' input.txt
```

## Feature flags

Match the rgx-core flags. Enable scripting backends at install time:

```bash
cargo install rgx-cli --features lua
cargo install rgx-cli --features javascript
cargo install rgx-cli --features rhai
cargo install rgx-cli --features wasm
cargo install rgx-cli --features all-languages
```

## Documentation

- **[CLI Guide](https://github.com/rdje/rgx/blob/main/docs/CLI_GUIDE.md)** — 20+ examples covering every flag
- **[The RGX Book](https://github.com/rdje/rgx/tree/main/book/src)** — the full engine reference

## License

Apache-2.0.
