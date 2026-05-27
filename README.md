# xgrep

Excel-aware grep — search `.xlsx` files with [ripgrep](https://github.com/BurntSushi/ripgrep)-style UX.

```text
$ xgrep '张三' reports/
reports/2024.xlsx
  Sheet1!A2:1: 张三
  Sheet1!B3:1: 张三应收账款
  汇总!A12:1: 张三 [comment]
```

## Install (from source)

```bash
cargo install --path .
```

Requires Rust 1.75+.

## Quickstart

```bash
xgrep PATTERN [PATH...]
```

- Defaults to the current directory if no path is given.
- Pattern is a regex (`-F` for fixed-string).
- Smart case is on by default: lowercase pattern → case-insensitive; any uppercase → case-sensitive.

## Common flags (rg-aligned)

| Flag | Meaning |
|---|---|
| `-i` / `-S` / `-s` | force insensitive / smart / sensitive |
| `-F` | literal string |
| `-w` | whole-word match |
| `-v` | invert |
| `-c` | per-file `path:count` |
| `-l` | only print paths with matches |
| `--json` | NDJSON event stream |
| `--color auto\|always\|never` | color control |
| `--glob 'PATTERN'` | filter file paths |
| `-j N` | threads |

## Excel-specific flags

| Flag | Meaning |
|---|---|
| `--formula` | also search formula text (e.g. `=SUM(...)`) |
| `--no-hidden` | skip hidden sheets / rows / columns |
| `--no-comments` | skip cell notes / comments |
| `--sheet GLOB` | only search sheets whose name matches |
| `--layers` | always print the layer tag (even `[display]`) |

By default, xgrep searches: cell **display values** + cached **formula results** + **comments** on hidden and visible content.

## Output format

```
<file path>
  <Sheet>!<Cell>:<offset>: <text> [optional layer tag]
```

- `Cell` is in A1 notation (e.g. `B3`, `AA17`).
- `offset` is **1-indexed Unicode character** position of the match start within the cell text.
- Layer tag is omitted for `display`; shown as `[cached]`, `[formula]`, `[comment]` otherwise.

## JSON mode

Emits one JSON object per line (NDJSON). Events:
- `begin` — start of a file's match block
- `match` — a single cell match
- `end` — end of a file's match block, includes stats
- `error` — per-file error (the file is skipped, scan continues)

Submatch `start` / `end` are **0-indexed Unicode character** offsets, half-open.

## Development

- Run tests: `cargo test`
- Generate bench fixtures: `cargo xtask gen-benches`
- List fixtures + sizes: `cargo xtask list-fixtures`
- Run benches (local only): `cargo bench`
- Spot-check memory: `cargo xtask measure-memory --fixture sst_heavy_low_hit`

## Troubleshooting

If a search produces fewer matches than `xgrep v0.1` did on the same file, set
`XGREP_DISABLE_FAST_PATH=1` and rerun. If the results match, the bug is in the
v0.2 fast-path — please open an issue with the offending fixture attached.

## Exit codes (rg-aligned)

| Code | Meaning |
|---|---|
| 0 | at least one match |
| 1 | no matches, no fatal error |
| 2 | fatal error (invalid regex, CLI mistake) |

Per-file errors (encrypted/corrupt xlsx, permission denied) are reported on stderr and do **not** alone cause exit 2.

## Known limitations

- Only `.xlsx` (no `.xls`, `.xlsb`, `.ods`, `.csv`). Reader is trait-ready for future formats.
- No encrypted-xlsx decryption: encrypted files are reported and skipped.
- Custom number formats not understood by calamine fall back to the raw numeric value.
- Stream-friendly large-file mode (>500 MB) is not yet implemented; xlsx files are loaded into memory.
- No context lines (`-A` / `-B` / `-C`) — semantics for "cell context" deferred to v0.2.
- No stdin input — xlsx requires random access.

## License

MIT OR Apache-2.0
