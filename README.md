# xgrep

Excel-aware grep — search `.xlsx`, `.csv`, and `.tsv` files with [ripgrep](https://github.com/BurntSushi/ripgrep)-style UX.

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
| `-A N` / `--after-context N` | show N rows after each match (xlsx: same sheet) |
| `-B N` / `--before-context N` | show N rows before each match |
| `-C N` / `--context N` | shorthand for `-A N -B N` |
| `-E ENC` / `--encoding ENC` | CSV/TSV decoding override (e.g. `gbk`, `utf-16le`); xlsx ignores |

## Excel-specific flags

| Flag | Meaning |
|---|---|
| `--formula` | also search formula text (e.g. `=SUM(...)`) |
| `--no-hidden` | skip hidden sheets / rows / columns |
| `--no-comments` | skip cell notes / comments |
| `--sheet GLOB` | only search sheets whose name matches |
| `--layers` | always print the layer tag (even `[display]`) |

By default, xgrep searches: cell **display values** + cached **formula results** + **comments** on hidden and visible content.

## CSV / TSV

xgrep auto-walks `.csv` (comma) and `.tsv` (tab) alongside `.xlsx`. CSV cells use A1 addresses just like xlsx, but the `Sheet!` prefix is suppressed (CSV has no sheet concept):

```
$ xgrep TARGET data.csv
data.csv
  A2:1: TARGET
  B5:3: my TARGET row
```

Encoding defaults to UTF-8 with BOM auto-detect (covers Excel-exported CSV on Windows). Override with `-E`:

```bash
xgrep '错误码' --encoding gbk logs.csv
xgrep header --encoding utf-16le exported.csv
```

Valid encoding labels follow the [WHATWG spec](https://encoding.spec.whatwg.org/) — unknown labels exit with code 2 before any file is opened. CSV parsing follows RFC 4180 quoting (`"foo,bar",baz` is two fields, not three).

## Context lines

rg-aligned `-A` / `-B` / `-C` show N rows around each match:

```
$ xgrep TARGET -C 1 data.csv
data.csv
  A1: row1 [context]
  A2:1: TARGET
  A3: row3 [context]
  --
  A7: row7 [context]
  A8:1: TARGET
  A9: row9 [context]
```

- For xlsx, "rows" means the matched cell's sheet rows; context never crosses sheets (`--` is inserted at sheet boundaries).
- For CSV/TSV, "rows" means file lines.
- Disjoint context spans within the same sheet/file are separated by `--`.
- `--count` (`-c`) reports match count, not output-line count — context lines do not inflate the count.
- `--json` adds `{"type":"context",...}` and `{"type":"separator","data":null}` events; existing events are unchanged.

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

- Only `.xlsx`, `.csv`, `.tsv` (no `.xls`, `.xlsb`, `.ods` — deferred to v0.4).
- No encrypted-xlsx decryption: encrypted files are reported and skipped.
- Custom number formats not understood by calamine fall back to the raw numeric value.
- Stream-friendly large-file mode (>500 MB) is not yet implemented; xlsx files are loaded into memory.
- No stdin input — xlsx requires random access.

## License

MIT OR Apache-2.0
