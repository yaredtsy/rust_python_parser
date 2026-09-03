# Exam 01 — Source and positions

Answer from your own output where the question says "run it". Everything else,
answer from memory first, then check.

---

## Recall

**1.** Why does ty use byte offsets instead of `(line, column)`? Give the
*structural* reason, not "because it is faster".

**2.** `TextRange` is 8 bytes and `Copy`. Name two things that become cheap
because of that, which are awkward in parso.

**3.** Your `parser.py` drops nodes that share an identical 4-tuple position
(quirk 10). What is the Rust equivalent, and why is it cheaper than in Python?

**4.** What is the difference between `ruff_db::source::line_index(db, file)`
and `LineIndex::from_source_text(text)`? When must you use the second one?

**5.** Your wire format wants 1-based lines and 0-based columns. `LineColumn`
gives you two `OneIndexed` values. Write the four expressions you need for
`line`, `column`, `end_line`, `end_column`.

---

## The encoding question

**6.** Which `PositionEncoding` does `LineIndex::line_column` use internally,
and what is the one-line implementation of that encoding? Cite the file.

**7.** For `python/unicode.py` line 1 — `def café(número):` — the `(` sits at:

- byte offset 9 within the line
- character offset 8
- UTF-16 offset 8

Which number does `line_column` report as the column, and which number does
parso report? Why does UTF-16 agree with characters here but not on line 2?

**8.** Line 2 is `    """Résumé 🎉 of the thing."""`. Give its length in all three
encodings. Which one is `end_column` for a range covering the whole line?

**9.** You are asked to also serve LSP `textDocument/hover`, which uses UTF-16
positions by default. Which method do you call, and what changes in your
`to_position` helper?

---

## The invisible bytes

**10.** `python/bom.py` starts with a 3-byte BOM. At what byte offset does `def`
begin?

**11.** Run both of these for offset 3 in that file:

```
line_column(3, source).column.to_zero_indexed()
source_location(3, source, Utf32).character_offset.to_zero_indexed()
```

What are the two values, why do they differ, and which one matches parso?

**12.** One of those two methods is documented as supporting *bidirectional*
mapping (offset → location → offset). Which one, and why does the BOM handling
make the other one unsuitable for that?

---

## Predict, then run

**13.** `python/tabs.py` line 3 is `\t\treturn tabbed(x - 1)`. What column does
`return` start at? Predict, then run. If you guessed 8 or 16, what assumption
were you making?

**14.** `python/crlf.py` line 1 is `def crlf_fn(x):\r\n`. What is the
`end_column` of the range `0..15`? What would `end_column` be for `0..16`, and
why would a node range never be `0..16`?

**15.** Which of the five fixture files would a positions test suite made only
of `ascii.py` fail to protect you against? List what each of the other four
catches.

---

## Practical

**16.** Do the loop A / loop B timing from step 6. Write down the ratio, the
file size, and the number of conversions. Then answer: in the real pipeline,
what makes this cost disappear almost entirely?

**17.** Write one assertion you could put in your test suite that would have
caught a `get()`-instead-of-`to_zero_indexed()` mistake. It must fail on the
buggy version — an assertion that passes either way is worse than none.
