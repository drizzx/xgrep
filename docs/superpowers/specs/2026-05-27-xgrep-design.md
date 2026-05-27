# xgrep — Excel-aware grep（设计稿 v0.1）

- **状态：** Draft，待用户评审
- **日期：** 2026-05-27
- **作者：** Drizzx + Claude（brainstorming）
- **范围：** v0.1 MVP 的需求、架构、模块、错误模型、测试与发布契约

---

## 1. 动机

`ripgrep`（rg）在「在一堆文件里快速找一行文本」这件事上几乎是终态体验。但 xlsx 不能进入这个体验：

- xlsx 是 zip 包裹的 XML，文件直接喂给 rg 只会得到一堆 XML 噪声
- Excel 「显示出来的内容」不等于「文件里存的字节」——日期可能是序号 `45439`，数字可能带千分位，公式 `=SUM(...)` 与其缓存值 `42` 是两份数据
- 命中后用户想知道的不是字节偏移，而是「哪个工作表的哪个单元格」

**xgrep** 是面向 .xlsx 的、贴近 rg UX 的命令行搜索工具。

## 2. 用户与场景

- **用户画像：** 命令行熟练用户、数据分析师、开发者（个人/桌面单机使用）
- **典型场景：** 在一个目录树下找「某个值/某个客户名/某串编号在哪个表的哪个格」
- **非目标：** GUI 工具、面向非技术用户的桌面产品、CI 批量审计专用流水线（虽然其输出可被脚本消费）

## 3. 核心设计决定（拍板项）

| 决策 | 选择 | 备择 | 选择理由 |
|---|---|---|---|
| 实现语言 | Rust | Go / Python | 最贴近 rg 依赖栈，能直接复用 `regex` / `ignore` / `termcolor`；单静态二进制；calamine 是 Rust 生态成熟 xlsx 解析库 |
| 匹配最小单位 | cell | row / 整 sheet | 心智模型最简单；输出坐标直接是 Excel A1；和 rg 的 line 概念有对应关系 |
| 搜索语义 | regex + smart case（rg 同款默认） | literal / 结构查询 | 与 rg 习惯零摩擦；smart case 已被证明是好默认 |
| 文件格式范围 | 仅 `.xlsx`（含 `.xlsm`） | xls/csv/ods 全家桶 | MVP 收口；reader 用 trait 抽象，留扩展点 |
| 默认匹配池 | 显示值 + 公式缓存值 + 批注 + 隐藏内容 | 仅显示值 | 防止「我肉眼看得到却搜不到」的挫败；隐藏内容对审计场景重要 |
| 公式文本 | 默认**不**搜，`--formula` 开启 | 默认搜 | 多数用户搜的是「数据」，不是「表达式」；避免误命中 |
| 坐标格式 | `Sheet!A3` (Excel A1 notation) | `row:col` 数字 | Excel Name Box 直接识别，方便跳转 |
| 库 vs CLI | 单 crate，搜索逻辑放 `src/lib.rs`，binary 只是薄壳 | 拆 lib + bin 双 crate | 当前需求只 CLI，避免过早架构；同时不堵未来 library 化的路 |

## 4. CLI 表面

### 4.1 命令形态

```
xgrep [OPTIONS] PATTERN [PATH...]
```

无 PATH 时默认搜当前目录递归。

### 4.2 rg 同款开关

| 开关 | 含义 |
|---|---|
| `-i, --ignore-case` | 强制大小写不敏感 |
| `-S, --smart-case` | 默认开启：纯小写 pattern 不敏感，含大写敏感 |
| `-F, --fixed-strings` | PATTERN 当字面量 |
| `-w, --word-regexp` | 仅匹配整词 |
| `-v, --invert-match` | 反向匹配（输出不含命中的 cell） |
| `-e, --regexp <PAT>` | 多模式（可重复） |
| `-c, --count` | 每文件输出一行 `path:count`（rg 同款） |
| `-l, --files-with-matches` | 只输出有命中的文件路径 |
| `--json` | 流式 JSON Lines 输出（结构见 §4.5） |
| `--color <when>` | `auto` (默认) / `always` / `never` |
| `-j, --threads <N>` | 默认 = CPU 核数 |
| `--glob <GLOB>` | 路径 glob 过滤；复用 `ignore::overrides::OverrideBuilder` |

### 4.3 xgrep 专属开关

| 开关 | 含义 |
|---|---|
| `--formula` | 把公式文本（如 `=SUM(A1:A10)`）纳入匹配池（默认关） |
| `--no-hidden` | 跳过隐藏 sheet/row/col（默认包含） |
| `--no-comments` | 跳过批注/notes（默认包含） |
| `--sheet <GLOB>` | 仅搜匹配名字的工作表，如 `--sheet 'Q*'` |
| `--layers` | 强制对**所有**命中输出层级标签（含 `[display]`）；默认行为是仅在层级 ≠ display 时输出标签 |

### 4.4 默认人类输出

```
reports/2024.xlsx
  Sheet1!B3:5: 张三应收账款
  Sheet1!D7:5: 张三离职
  汇总!A12:5: 张三 [comment]
  汇总!C12:5: =VLOOKUP(张三,...) [formula]   ← 仅当 --formula 开启时
```

- 文件名独占一行（彩色 = 紫）
- 命中行缩进两空格；`Sheet!Cell:char_offset:` 前缀（蓝/绿/灰），后接命中文本（黄高亮匹配子串）
- `char_offset` 是 cell 文本内匹配起始位置，**1-indexed Unicode 字符**（不是字节）。多次命中同一 cell 时会输出多行，offset 不同。与 rg `--column` 的 1-indexed 习惯一致；但因 Excel 文本是 Unicode 主导，用字符而非字节，避免 CJK 字节偏移让用户困惑
- 层级标签：`display` 不显示；`cached` / `formula` / `comment` 加方括号后缀

### 4.5 `--json` 输出（NDJSON，rg 同款事件流）

```json
{"type":"begin","data":{"path":"reports/2024.xlsx"}}
{"type":"match","data":{
  "path":"reports/2024.xlsx",
  "sheet":"Sheet1",
  "cell":"B3",
  "layer":"display",
  "text":"张三应收账款",
  "submatches":[{"match":{"text":"张三"},"start":0,"end":2}]
}}
{"type":"end","data":{"path":"reports/2024.xlsx","stats":{"matches":4,"sheets_scanned":3}}}
```

每行一个 JSON 对象，UTF-8，`\n` 分隔。`type` 取值：`begin` / `match` / `end` / `summary`（末尾全局总结）。

JSON `submatches[].start` / `end` 是 **0-indexed Unicode 字符**半开区间 `[start, end)`。与 rg JSON 输出形状一致（rg 用字节，我们用字符，原因同 §4.4）。

## 5. 架构与模块

### 5.1 模块拆分

```
src/
  lib.rs          # 内部组织用：把 search(Config) -> Receiver<MatchEvent> 留在 lib 里，main 只做 IO/CLI；v0.1 不对外发布 library API（避免过早承诺接口）
  config.rs       # Config 结构 + 默认值 + smart case 推断
  walker.rs       # ignore::WalkBuilder 遍历 -> xlsx 路径流
  reader.rs       # calamine 单文件读, 抽出 (sheet, cell, layer, text)
  matcher.rs      # 包装 regex::Regex, 提供 is_match / find_iter
  printer.rs      # 人类彩色输出 + JSON Lines 输出
  worker.rs       # rayon 文件级并行池
  main.rs         # clap -> Config -> 启动 search -> printer
```

### 5.2 核心类型

```rust
pub struct Config {
    pub pattern: Pattern,
    pub paths: Vec<PathBuf>,
    pub layers: LayerSet,            // bitflags
    pub include_hidden: bool,
    pub sheet_glob: Option<Glob>,
    pub file_glob: Option<Glob>,
    pub threads: usize,
    pub output: OutputMode,          // Pretty | Json | CountOnly | FilesOnly
    pub color: ColorChoice,
    pub invert: bool,
}

bitflags! {
    pub struct LayerSet: u8 {
        const DISPLAY = 0b0001;
        const CACHED  = 0b0010;
        const COMMENT = 0b0100;
        const FORMULA = 0b1000;
    }
}

pub enum MatchEvent {
    FileBegin { path: PathBuf },
    Match {
        path: PathBuf,
        sheet: String,
        cell: String,              // A1 notation, e.g. "B3"
        layer: Layer,
        text: String,
        submatches: Vec<Submatch>,
    },
    FileEnd { path: PathBuf, stats: FileStats },
    Error { path: PathBuf, err: SearchError },
}

pub struct Submatch { pub start: usize, pub end: usize }
pub struct FileStats { pub matches: u64, pub sheets_scanned: u32 }
```

### 5.3 数据流（一次 `xgrep '张三' reports/`）

```
   CLI args  ─►  main.rs (clap → Config, smart-case 推断)
                  │
                  ▼
              walker.rs  ─►  ignore::Walk + 扩展名/glob 过滤
                  │
                  ▼ 路径流
              worker.rs  ─►  rayon::par_iter (文件级)
                  │
                  ▼ per-file
       ┌──────────┴──────────┐
       ▼                     ▼
   reader.rs              matcher.rs
   (calamine →             (regex find_iter
    sheet/cell/layer)       per cell text)
       └──────────┬──────────┘
                  ▼ per-file 事件块 (Begin..Match*..End)
              mpsc channel
                  │
                  ▼ 主线程
              printer.rs  ─►  stdout
```

**事件顺序保证：** 每个 worker 把单文件的 `FileBegin..Match*..FileEnd` 累积成一个块，整块发到 channel；printer 整块输出，文件之间互不交错（rg 同款）。

### 5.4 Excel 语义映射（reader 实现要点）

| 层 | calamine API |
|---|---|
| display | `Range::get_value` 配 `cell.formatted_value()`（应用 number format） |
| cached | 公式 cell 的 `Data::Float / String / DateTime` 即缓存值 |
| formula | `Range::formulas()` 或 cell-level formula 访问 |
| comment | `Reader::worksheet_comments()`，关联到被批注 cell |
| 隐藏检测 | `sheet.visibility()`、row/col 的 `hidden` 属性 |

### 5.5 并发模型

- **文件级并行**：rayon 默认线程池，每文件一个 task
- **文件内串行**：xlsx 是 zip + 共享 strings 表，多线程读同一文件没有 win
- **输出汇聚**：mpsc channel；printer 单线程消费，保证字节流原子性

## 6. 错误模型

### 6.1 错误枚举

```rust
pub enum SearchError {
    Open(io::Error),
    Parse(calamine::Error),
    Sheet { sheet: String, source: calamine::Error },
    InvalidRegex(regex::Error),
    Io(io::Error),
    Encrypted,                      // 加密 xlsx，特殊提示
}
```

### 6.2 隔离层级

| 错误 | 行为 | 进程退出码 |
|---|---|---|
| 正则编译失败 | 立即报错退出 | 2 |
| CLI 参数错误 | clap 退出 | 2 |
| 文件无法打开 | stderr 一行 `xgrep: <path>: <err>`，继续 | 见末行 |
| xlsx 整体解析失败 | 同上，整文件跳过 | 见末行 |
| 单 sheet 解析失败 | stderr warning，文件内其他 sheet 继续 | 见末行 |
| 加密 xlsx | stderr `xgrep: <path>: encrypted workbook (not supported)`，跳过 | 见末行 |
| stdout broken pipe | 静默退出 | 0 |

### 6.3 退出码契约（rg 同款）

| 状态 | 码 |
|---|---|
| 有任何匹配 | 0 |
| 无匹配，无错误 | 1 |
| CLI/正则/参数错误 | 2 |

错误存在但仍有匹配 → 0（已在 stderr 报告）。

### 6.4 信号

- `SIGINT` (Ctrl-C)：关闭 channel → workers 见 send 失败退出 → 主线程 flush printer → exit 130

## 7. 边界情况显式处理

| 场景 | 行为 |
|---|---|
| 空 xlsx / 全空 sheet | 安静通过，0 matches |
| 循环引用公式 | 不求值；读 calamine 给的 cached 值即可 |
| 超大文件（>500MB） | MVP 全反序列化；README 标注内存上限；流式读延后到 v0.3 |
| 加密 xlsx | 友好错误，跳过 |
| stdin 输入 (`-`) | 显式不支持；help 文档说明（xlsx 需随机访问） |
| 极宽行/极长 cell | regex 引擎自适应；无需特别处理 |
| 自定义 number format calamine 解不出 | 显示原始数字；记入 README 已知限制 |
| 富文本 cell（一个 cell 多 run） | 合并为单字符串后匹配 |
| 合并单元格 | 仅 anchor cell 出现一次（xlsx 原生语义） |

## 8. 测试策略

### 8.1 金字塔

| 层 | 工具 | 覆盖 |
|---|---|---|
| 单元 | `cargo test` | matcher smart-case 推断、cell A1 编解码、glob、Config 默认值 |
| 集成（黑盒） | `assert_cmd` + `insta` | end-to-end CLI；stdout/stderr/exit code 快照 |
| 数据正确性 | `tests/fixtures/` 真实 xlsx | 见 §8.2 |
| 性能回归 | `criterion` | 1000 sheets × 1000 rows；100 文件并发 |
| 模糊 | `cargo-fuzz` | 损坏 xlsx 不 panic，错误隔离正确 |

### 8.2 Fixture 集（v0.1 必备）

```
tests/fixtures/
  basic.xlsx                 # 普通文本 + 数字
  dates.xlsx                 # 多种日期格式（1900 vs 1904 闰年）
  formulas.xlsx              # SUM/VLOOKUP/循环引用/#N/A
  richtext.xlsx              # 富文本（一个 cell 多 run）
  merged.xlsx                # 合并单元格
  hidden.xlsx                # 隐藏 sheet/row/col
  comments.xlsx              # 批注（threaded + legacy）
  shared_strings_heavy.xlsx  # share-strings 优化基准
  encrypted.xlsx             # 期望 graceful 错误
  corrupt.xlsx               # 故意截断的 zip → 整文件跳过
  empty.xlsx                 # 空工作簿
```

### 8.3 CI

- `cargo fmt --check`
- `cargo clippy -- -D warnings`
- `cargo test --all-features`
- `cargo bench` 仅在 PR 标签 `bench` 时跑

## 9. 范围划定

### 9.1 v0.1 MVP（必交付）

- `xgrep PATTERN PATH...` 基本搜索
- 第 4 节列出的所有 rg 同款开关与 xgrep 专属开关
- 第 5 节的全部模块
- 第 6 节的全部错误处理与退出码契约
- 第 8.2 的 11 个 fixtures + 集成测试
- 彩色输出 + JSON Lines 输出
- 文件级并行
- README（用法 + 已知限制）

### 9.2 明确**不**进 v0.1

- share-strings 预过滤优化 → v0.2 性能里程碑
- 流式超大文件读 → v0.3，按需
- 上下文 `-A/-B/-C` → 语义待讨论（cell 的「上下文」是相邻 cell 还是相邻 row？）
- 加密 xlsx 解密
- 非 xlsx 格式（.xls/.csv/.ods）—— reader 用 trait 抽象，扩展点已留
- stdin 输入
- 复杂查询语言（列选择、聚合）
- TUI / Web UI

### 9.3 性能验收门槛（非硬性，作为 v0.1 完成判据参考）

| 场景 | 目标 |
|---|---|
| 100 个 1MB xlsx，4 线程 | < 3 s |
| 单个 100MB xlsx | < 5 s |
| 1GB 数据集扫描时常驻内存 | < 200 MB |

## 10. 项目骨架

```
xgrep/
  Cargo.toml
  src/
    main.rs
    lib.rs
    config.rs
    walker.rs
    reader.rs
    matcher.rs
    printer.rs
    worker.rs
  tests/
    fixtures/        # xlsx 文件（直接 commit；体积小，无需 git-lfs）
    it/              # integration tests
    snapshots/       # insta 快照
  benches/
  README.md
  .github/workflows/ci.yml
```

主要依赖（`Cargo.toml`）：

```toml
[dependencies]
calamine = "0.26"
regex = "1.10"
ignore = "0.4"
termcolor = "1.4"
clap = { version = "4", features = ["derive"] }
rayon = "1.10"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
bitflags = "2"
globset = "0.4"
crossbeam-channel = "0.5"
anyhow = "1"
thiserror = "2"

[dev-dependencies]
assert_cmd = "2"
insta = "1"
predicates = "3"
criterion = "0.5"
```

## 11. 未决问题（spec 评审时确认）

- 输出里 `Sheet!Cell:char_offset:` 的 `char_offset` 是否保留？（设计稿默认保留）
- 富文本 cell 跨 run 的命中是否要在输出里显示 run 边界？（默认不显示）
- 是否需要 `--type-list` 等同 rg 的类型管理命令？（v0.1 不要）

---

**下一步：** 用户评审本 spec 后，进入 superpowers:writing-plans 生成实现计划。
