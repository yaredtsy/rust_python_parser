# 01 — Source and positions

**Goal:** you can take any AST node and produce the exact `position` object your
wire format needs, and you can defend every number in it.

This is the smallest exercise with the highest bug density in the whole port.
Every node you ever emit carries four numbers from here. Get them wrong and the
error surfaces months later as "v-noc highlights the wrong line", with nothing
in the logs.

---

## How this exercise is organised

Four objects, one file each, in the order you meet them. Same shape as exercise
00: lesson → API → worked examples → exercise → its own exam → answers.

| | file | object | what it gives you |
|---|---|---|---|
| 1 | [`01-object-textsize.md`](01-object-textsize.md) | `TextSize`, `TextRange`, `Ranged` | how ty points at code |
| 2 | [`02-object-sourcetext.md`](02-object-sourcetext.md) | `source_text`, `SourceText` | the characters — and your first cached query |
| 3 | [`03-object-lineindex.md`](03-object-lineindex.md) | `LineIndex`, `LineColumn`, `OneIndexed`, `PositionEncoding` | ★ offsets → line/column |
| 4 | [`04-object-sourcecode.md`](04-object-sourcecode.md) | `SourceCode` | the convenience wrapper, and `slice` |
| 5 | [`05-putting-it-together.md`](05-putting-it-together.md) | — | `src/position.rs`, three tests, the fixtures |

Then [`exam.md`](exam.md) for the whole exercise.

**File 3 is the one that matters.** Files 1, 2 and 4 are small. If you only have
an hour, read 1 and 3.

---

## Read first

- `tutorial/04-positions-and-text.md` — the whole chapter, it is short
- `plan/02-mapping/01-parso-to-ruff-ast.md` §"Positions: the `LineIndex` bridge"

---

## The mental model, in one page

Jedi/parso hands you `(line, column)` on every node. Ruff hands you **two
integers**: byte offsets into the file, start and end.

```
source:   d  e  f     g  r  e  e  t  (  n  a  m  e  )  :  \n     …
offset:   0  1  2  3  4  5  6  7  8  9 10 11 12 13 14 15 16     …
                     └────────────────┘
                     TextRange { start: 4, end: 9 }   ← the name `greet`
```

`TextSize` is a `u32`. `TextRange` is two of them — **eight bytes, `Copy`,
comparable, hashable**. That is the entire position representation in ty.

Three consequences, and they are the point:

1. **Nothing needs to know where the newlines are.** parso cannot produce a
   position without having split the file into lines; ruff can, because the
   offset *is* the position. Line/column becomes a rendering concern, computed
   at the edge, once.
2. **Ranges are cheap to compare and store.** "Is this node inside that one?" is
   two integer comparisons. "Have I seen this position?" is an
   `FxHashSet<TextRange>` — the position-dedup quirk from your `parser.py`
   (`plan/00-orientation/01`, quirk 10) becomes one hash lookup instead of a
   4-tuple comparison.
3. **Line/column costs a lookup.** You need a `LineIndex`: a sorted array of the
   byte offset where each line starts. Binary search gives the line; the column
   is the distance from that line's start. Which is why you build it **once per
   file**, never per node.

```
LineIndex for ascii.py     line_starts = [0, 17, 41, 42, 43, 58, 77]
                                          ▲   ▲
offset 20  →  binary search finds line 2 ──┘   └── line 2 starts at byte 17
              column = characters between byte 17 and byte 20 = 3
```

### Your wire format

From `plan/00-orientation/01`: **line is 1-based, column is 0-based** — parso's
convention, and v-noc downstream depends on it.

```jsonc
"position": { "line": 1, "column": 0, "end_line": 3, "end_column": 12 }
```

Ruff's `LineColumn` gives you **both fields as `OneIndexed`**. So `line` passes
through and `column` needs converting. That asymmetry is the single most common
off-by-one in this port, and file 3 is where you deal with it properly.

### The `[check]` the plan left open

`plan/02-mapping/01` flags character vs UTF-8 vs UTF-16 columns as a real parity
risk and tells you to check which method matches parso. **It is answered in file
3**, with the source line: `line_column` counts characters. You still have to
prove it on `unicode.py`, because a claim you have not tested is a claim
somebody else made.

---

## The fixtures

```
python/
├── ascii.py ....... plain. 100 bytes, 100 characters. your baseline.
├── unicode.py ..... 88 bytes, 79 characters. accented identifiers, an emoji.
├── tabs.py ........ tab-indented. does a tab count as 1 column or 4?
├── crlf.py ........ CRLF line endings. where does a line end?
└── bom.py ......... starts with a UTF-8 BOM. 3 invisible bytes.
```

Each one exists to break a different assumption:

| fixture | what it catches |
|---|---|
| `unicode.py` | byte vs character columns — the big one |
| `bom.py` | an off-by-one on the first line only |
| `crlf.py` | line-terminator handling |
| `tabs.py` | tab expansion, if someone "helpfully" adds it |

`ascii.py` is the only file where byte offset, character offset and column all
agree — which is why **a test suite made only of ASCII files proves nothing
about positions**, no matter how large it is. Two hundred real files from a
Western codebase are mostly ASCII, so the M2 gate in
`plan/04-build/02-milestones.md` needs these four added deliberately.

---

## Before you start

Do the crate split from exercise 00, file 09, if you have not: `src/lib.rs`,
`src/db.rs`, `src/bin/pylspt-dev.rs`. This exercise adds `src/position.rs`, and
that only works if you have a library to add it to.

---

## Done when

- [ ] `to_position` converts any `TextRange` to your wire shape
- [ ] three unit tests pass, each rejecting a different wrong implementation
- [ ] you can state which encoding `line_column` uses, and cite where you saw it
- [ ] you know why `line_column` and `source_location` disagree on `bom.py`
- [ ] you can answer the tab and CRLF questions from your own output
- [ ] you measured "index per node" vs "index once" and wrote the ratio down
- [ ] your database path uses `ruff_db::source::line_index(db, file)`

---

→ Start: [`01-object-textsize.md`](01-object-textsize.md)
