# CLI Guide

The `rgx` command-line tool provides direct access to the regex engine from your terminal. It supports pattern matching, replacement, file scanning, structured event output, and code-block execution.

## Installation

```bash
cargo install rgx-cli
```

Or build from source:

```bash
cargo build --release -p rgx-cli
```

## Basic usage

```bash
rgx '<pattern>' '<text>'
```

```bash
$ rgx '\d+' 'abc 42 xyz 99'
Match: 42 (4..6)
Match: 99 (12..14)
```

When `<text>` is omitted, input is read from stdin:

```bash
echo "hello 42 world" | rgx '\d+'
```

## Common flags

### --color

Control colorized output:

```bash
rgx --color always '\d+' 'abc 42'    # force color
rgx --color never  '\d+' 'abc 42'    # no color
rgx --color auto   '\d+' 'abc 42'    # detect terminal (default)
```

### --file

Read input from a file:

```bash
rgx --file app.log 'ERROR.*'
```

### --line-mode

With `--file`, match each line independently and show line numbers:

```bash
rgx --file app.log --line-mode 'ERROR\s+(.*)'
```

Output:

```text
42: ERROR connection refused
89: ERROR timeout exceeded
```

### --replace

Replace matches with a string and print the result:

```bash
$ rgx --replace 'X' '\d+' 'abc 42 xyz 99'
abc X xyz X
```

Template substitution works:

```bash
$ rgx --replace '$2-$1' '(\w+)\s(\w+)' 'hello world'
world-hello
```

### --only-matching

Print only the matched text, one per line:

```bash
$ rgx -o '\d+' 'abc 42 xyz 99'
42
99
```

### --invert-match

Print lines that do NOT match (line-mode only):

```bash
rgx --file app.log --line-mode --invert-match 'DEBUG'
```

### --count

Print the number of matches instead of the matches themselves:

```bash
$ rgx --count '\d+' 'a1 b2 c3'
3
```

### --json

Output matches as JSON:

```bash
$ rgx --json '\d+' 'abc 42 xyz 99'
[{"start":4,"end":6,"text":"42"},{"start":12,"end":14,"text":"99"}]
```

### --context

Show N lines of context before and after each match (line-mode only):

```bash
rgx --file app.log --line-mode --context 2 'ERROR'
```

### --recursive

Scan directories recursively when `--file` points to a directory:

```bash
rgx --file src/ --recursive --line-mode 'TODO'
```

## Code execution flags

### --mode

Set the execution mode:

```bash
rgx --mode safe '(\d+)(?{lua:return tonumber(arg[1]) > 0})' '42'
rgx --mode full '(\d+)(?{native:check})' '42'
rgx --mode pure '\d+' '42'    # default
```

### --var

Set a host variable for code blocks:

```bash
rgx --mode safe --var threshold=50 \
    '(\d+)(?{lua:return tonumber(arg[1]) > tonumber(vars["threshold"])})' '100'
```

Multiple variables:

```bash
rgx --mode safe \
    --var env=prod \
    --var max=100 \
    '(?{lua:return vars["env"] == "prod"})' 'test'
```

### --var-json

Set a typed variable as JSON:

```bash
rgx --mode safe --var-json 'config={"threshold":50,"enabled":true}' \
    '(?{lua:return true})' 'test'
```

### --wasm-module

Register a WASM module for `(?{wasm:...})` patterns:

```bash
rgx --mode safe --wasm-module validator=/path/to/module.wasm \
    '(?{wasm:validator:check})' 'input'
```

## Output control flags

### --events

Print structured match events to stderr:

```bash
$ rgx --events '\d+' 'abc 42'
[event] MatchAttemptStarted { position: 0 }
[event] MatchAttemptCompleted { position: 0, matched: false }
[event] MatchAttemptStarted { position: 1 }
...
Match: 42 (4..6)
```

Events go to stderr, matches go to stdout, so you can pipe normally:

```bash
rgx --events '\d+' 'abc 42' 2>events.log | process_matches
```

### --numeric

Collect and print numeric code block results:

```bash
rgx --mode safe --numeric '(\d+)(?{lua:return tonumber(arg[1]) * 2})' '21 42'
```

### --replace-with-code

Use code block replacement values as the replacement text:

```bash
rgx --mode safe --replace-with-code \
    '(\w+)(?{lua:return string.upper(arg[1])})' 'hello world'
```

### --stats

Print match statistics to stderr at the end:

```bash
$ rgx --stats '\d+' 'a1 b2 c3'
Match: 1 (1..2)
Match: 2 (4..5)
Match: 3 (7..8)
[stats] matches: 3, input_length: 8, pattern: \d+
```

### --show-details

Include branch numbers and code block details in output:

```bash
rgx --mode full --show-details '(\d+)|([a-z]+)' 'abc 42'
```

## Debugging flags

### --debug / -d

Enable high-verbosity output:

```bash
rgx -d '\d+' '42'
```

### --trace / -t

Enable debug-level tracing (very verbose):

```bash
rgx -t '\d+' '42'
```

### --verbosity

Set verbosity level explicitly:

```bash
rgx --verbosity none   '\d+' '42'
rgx --verbosity low    '\d+' '42'
rgx --verbosity medium '\d+' '42'
rgx --verbosity high   '\d+' '42'
rgx --verbosity debug  '\d+' '42'
```

### --quiet

Suppress all trace/debug output:

```bash
rgx --quiet '\d+' '42'
```

### --trace-log

Route debug/trace output to `trace.log` instead of the terminal:

```bash
rgx --trace --trace-log '\d+' '42'
# Debug output goes to trace.log, matches go to stdout
```

## Examples

### Find all email addresses in a file

```bash
rgx --file contacts.txt --only-matching '[\w.+-]+@[\w.-]+\.\w{2,}'
```

### Count errors in a log

```bash
rgx --file /var/log/app.log --line-mode --count 'ERROR'
```

### Replace dates in a document

```bash
rgx --file report.txt --replace '$3/$2/$1' '(\d{4})-(\d{2})-(\d{2})'
```

### Find TODOs with context

```bash
rgx --file src/ --recursive --line-mode --context 2 'TODO|FIXME|HACK'
```

### Validate numbers with Lua

```bash
rgx --mode safe '(\d+)(?{lua:return tonumber(arg[1]) >= 18})' 'age: 21'
```

### JSON output for scripting

```bash
rgx --json --file data.csv '\d+\.\d{2}' | jq '.[].text'
```

## Exit codes

| Code | Meaning |
|------|---------|
| 0 | At least one match found |
| 1 | No matches found |
| 2 | Error (invalid pattern, file not found, etc.) |
