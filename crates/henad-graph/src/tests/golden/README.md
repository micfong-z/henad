# Golden files of the sheet writer

These four files hold the text that the author format's reference writer, `check.py`, writes for the format's four examples, Game of Life, SIR, boids and ants, with every comment line removed.
`the_rust_writer_agrees_with_check_py_apart_from_comments` in `../sheet_writer.rs` writes the same four sheets with the Rust writer, removes its comment lines too, and compares the two texts byte for byte.
The comments are left out of the comparison because they hold the sentences of the lines, and the Rust writer takes its sentences from the editor's final wording, which `check.py` predates.

## Procedure

`check.py` is the author format's reference checker and writer, a Python script kept with the format's design outside this repository.
The copy used has the SHA-256 `9879a44b6bb0472791fa27c9d8e7814620ed991d6ce11c46bfd8f700f720d02a` and ran under Python 3.15.0.

Each file was written from the sheet of the same name as it stood at commit `ea28d50`, before the Rust writer existed: `crates/henad-graph/sheets/game_of_life.toml`, and `sir.toml`, `boids.toml` and `ants.toml` in `crates/henad-graph/src/tests/sheets/`.
Each ran on a scratch copy outside the repository:

```sh
cp <sheet> scratch/<name>.toml
python3 check.py --write scratch/<name>.toml
grep -v '^#' scratch/<name>.toml > golden/<name>.toml
```

`--write` replaces the file with its canonical text.
`grep -v '^#'` removes every line that starts with `#`, which in a canonical text are the generated comments alone.
At `ea28d50` the four sheets already equalled `check.py`'s output apart from their comments.
The ants starter later took a note under its last line on its slower trail formation, which the format's example lacks, so the test drops every line's note before it writes the four sheets.
The four starters later took the ids `sheet_game_of_life`, `sheet_sir`, `sheet_boids` and `sheet_ants`, and the test takes `sheet_` off each id before it writes, so the golden files keep the examples' ids.
The four purposes and the ants starter's description of its trail strength were later reworded, and the new words were put into the golden files by hand.
The procedure's three commands, run on a copy of each edited file in place of the sheet, return the file unchanged.

## The rules the files follow

The rules are written out here so that a reader without `check.py` can audit a golden file by hand.

**Text.**
UTF-8 with `\n` line ends, ending in one `\n`.
The three header keys come first, in the order `format`, `format_version`, `kind`.
One blank line precedes every table header, and no other line is blank.

**Table order.**
Top-level tables follow the order of the sheet: `[sheet]`, `[world]`, `[time]`, `[[cells.property]]`, `[[agents]]` with its `[[agents.property]]`, `[ground]` and `[[ground.property]]`, `[[event]]`, `[[parameter]]`, `[[measure]]`, `[color.<thing>]` for the cells, then each agent kind, then the ground, `[[button]]`, `[[card]]`.
Declarations keep the author's order: properties, states, parameters, measures, buttons and cards appear as the sheet shows them.
A card's line tables `[card.line.<id>]` follow the card, in line id order by number, `l9` before `l10`, whatever the display order in `lines`.

**Key order.**
Within a table the keys follow the list for its table type, and a key the list leaves out follows in lexical order.
A table writes its inline keys first, then its sub-tables in key order.
A table with no inline key that holds sub-tables writes no header of its own, so `[cells]` stays implicit above `[[cells.property]]`.

| Table | Keys in order |
| --- | --- |
| `[sheet]` | `version`, `id`, `name`, `purpose`, `next_line` |
| `[world]` | `made_of`, `width`, `height`, `edges` |
| `[time]` | `tick_length`, `tick_unit` |
| `[[agents]]` | `id`, `name`, `count`, `max_count`, `property` |
| `[ground]` | `blocked`, `property` |
| a property | `id`, `name`, `type`, `states`, `marks`, `kept_per_tick`, `starts_as`, `note` |
| `starts_as` | `rule`, `key`, `base`, `property`, `state`, `value`, `otherwise`, `row`, `shape` |
| `row` | `state`, `chance`, `value`, `condition` |
| `shape` | `shape`, `state`, `center`, `size`, `angle` |
| `[[event]]` | `id`, `name`, `means` |
| `[[parameter]]` | `id`, `name`, `type`, `min`, `max`, `default`, `step`, `unit`, `format`, `means` |
| `[[measure]]` | `id`, `name`, `color`, `rule`, `over`, `property`, `state`, `event`, `value` |
| `[color.<thing>]` | `rule`, `property`, `shade_state`, `shades`, `scale`, `decades` |
| `[[button]]` | `id`, `name`, `does` |
| `[[button.does]]` | `rule`, `thing`, `properties`, `property`, `card`, `value` |
| `[[card]]` | `id`, `every`, `runs`, `lines`, then the line tables |
| a line or a phrase | `kind`, `name`, `key`, `property`, `new_state`, `start_state`, `mode`, `around`, `agent_kind`, `distance`, `include_me`, `open_only`, `pick`, `ranked_by`, `which`, `place`, `ground_property`, `event`, `test`, `left`, `right`, `tolerance`, `chance`, `rate`, `rate_unit`, `tries`, `low`, `high`, `length`, `formula`, `times`, `value`, `strength`, `minimum`, `maximum`, `amount`, `only`, `whose`, `condition`, `branches`, `summaries`, `items`, `row`, `otherwise`, `lines`, `note` |
| a chip | `param`, `result`, `part`, `property`, `my`, `my_new`, `their`, `state`, `relation` |
| a state | `id`, `name`, `color` |
| a summary | `id`, `name`, `summary`, `value` |
| a shade | `property`, `color` |
| a blocked state | `property`, `state` |

**Forms.**
A chip, a literal and a list of literals are inline, on one line.
A list of records with no phrase inside (`states`, `summaries`, `shades`, `blocked`) is an inline array with one element per line, each indented by two spaces and followed by a comma.
A phrase is a standard sub-table, such as `[card.line.l2.condition]`.
`row`, `items`, `shape`, `does` and the property lists are arrays of tables, and an item that is a chip is a table of its chip keys.
`branches` is an inline table in the state order of its property.
The form follows the schema and the value's form, never its length.

**Numbers.**
A whole number with a magnitude below 2^53 is written as an integer: `3`, `0`, `1024`, also where the value could hold a fraction, as a default of `1`.
Any other number is written in the shortest form that reads back to the same 64-bit float.
That form is plain notation when 10^-4 <= |x| < 10^16 (`0.3`, `0.0005`), and otherwise a mantissa, `e` and an exponent with no `+` and no leading zero (`1e-5`, `2.5e-7`).
`-0` is written `0`.
A key under a line or a start is a decimal integer.

**Strings.**
TOML basic strings.
`"` is written `\"`, `\` is written `\\`, a line feed `\n`, a tab `\t` and a carriage return `\r`.
Every other control character, U+0000 to U+001F and U+007F to U+009F, is written `\uXXXX` with upper-case hex digits.

**Colours.**
`"#RRGGBB"` in upper case, or `"#RRGGBBAA"` when alpha is not `FF`.

## Where the Rust writer goes further

The four examples hold none of the following, so the golden files say nothing of them, and `../sheet_writer.rs` tests each.

- `rate_unit_from_tick = true` follows `rate_unit` in a line or phrase, and is left out when false.
- A percent is the inline table `{ percent = 30 }`, its number under the number rule.
- A list of chips stays on one line, as a list of literals does.
  `check.py` writes a list whose every element is a table with one element per line.
- An `items` list that holds a literal is one inline array, its phrases written as inline tables, since an array of tables cannot hold a literal.
- A key that TOML does not read bare, such as a line id with a space, is written as a basic string.
- A control character in a generated comment is written as a space, so the comment stays on one line.
