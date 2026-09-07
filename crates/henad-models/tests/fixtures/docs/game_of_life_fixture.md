# Generating the Game of Life consistency fixture

Produces `crates/henad-models/tests/fixtures/*.txt`, the reference grids that `consistency_game_of_life.rs::matches_every_reference_fixture` compares against.


## Reference model

Wilensky, U. (1998). NetLogo Life model. http://ccl.northwestern.edu/netlogo/models/Life. Center for Connected Learning and Computer-Based Modeling, Northwestern University, Evanston, IL.

This model by Wilensky is licensed under the Creative Commons Attribution-NonCommercial-ShareAlike 3.0 license, so it isn't redistributed here. We'll walk through the steps to generate the reference grid from the original model.

## Model equivalence

Life's `go` matches Henad on all four aspects:

|               | NetLogo Life                            | Henad `GameOfLifeModel`                |
| ------------- | --------------------------------------- | -------------------------------------- |
| neighbourhood | `neighbors` (8 patches)                 | `NeighborhoodKind::Moore`              |
| update        | two passes, so simultaneous             | double-buffered `Grid2D`               |
| rule          | birth on exactly 3, death unless 2 or 3 | `(ALIVE, 2..=3) \| (DEAD, 3) => ALIVE` |
| edges         | world wraps in both directions          | toroidal                               |

## World setup

In _Settings_:

- **Location of origin**: Corner, Bottom Left.
- `max-pxcor` **63**, `max-pycor` **63**. The dialog should read `Torus: 64 x 64`.
- both **wrap** boxes checked

Patch size is irrelevant.

## Coordinates

Despite the origin being in the bottom-left corner, the vertical mirroring only happens in GUI rendering. During export Henad's `(x, y)` is the same as NetLogo's `(pxcor, pycor)`.

## Glider

### Procedure

Paste into the Code tab:

```netlogo
to setup-glider
  setup-blank
  ;; Glider at Henad origin (0,0), matching GLIDER in consistency_game_of_life.rs:
  ;;   .X.
  ;;   ..X
  ;;   XXX
  foreach [[1 0] [2 1] [0 2] [1 2] [2 2]] [ c ->
    ask patch (item 0 c) (item 1 c) [ cell-birth ]
  ]
end

to export-grid [ filename scenario steps ]
  if file-exists? filename [ file-delete filename ]
  file-open filename
  file-print (word "# engine: NetLogo " netlogo-version)
  file-print "# model: Life (Wilensky 1998), Models Library, unmodified"
  file-print (word "# scenario: " scenario)
  file-print (word "# width: " (max-pxcor - min-pxcor + 1))
  file-print (word "# height: " (max-pycor - min-pycor + 1))
  file-print (word "# steps: " steps)
  file-print "# neighbourhood: moore"
  file-print "# wrap: both"
  file-print (word "# generated: " date-and-time)
  ;; Ascending pycor, so Henad row y is NetLogo row y. See Coordinates above.
  let y min-pycor
  while [ y <= max-pycor ] [
    let x min-pxcor
    while [ x <= max-pxcor ] [
      file-type ifelse-value ([living?] of patch x y) [ "1" ] [ "0" ]
      set x x + 1
    ]
    file-print ""
    set y y + 1
  ]
  file-close
end
```

Then in the Command Center:

```netlogo
setup-glider
repeat 101 [ go ]
export-grid "gol_glider_64x64.txt" "glider" 101
```

This would usually appear in the same directory as the NetLogo model file. Move the result to `crates/henad-models/tests/fixtures/`.

Since the glider has a period of 4, 101 steps ensures that it will be in a different shape than the initial configuration.

## R-pentomino

The R-pentomino is a chaotic pattern that takes 1103 steps to stabilize.

### Procedure

With `export-grid` from the glider section, append the following into the Code tab:

```netlogo
to setup-r-pentomino
  setup-blank
  ;; R-pentomino at Henad origin (0,0), matching R_PENTOMINO in
  ;; consistency_game_of_life.rs:
  ;;   .XX
  ;;   XX.
  ;;   .X.
  foreach [[1 0] [2 0] [0 1] [1 1] [1 2]] [ c ->
    ask patch (item 0 c) (item 1 c) [ cell-birth ]
  ]
end
```

Then in the Command Center:

```netlogo
setup-r-pentomino
repeat 500 [ go ]
export-grid "gol_r_pentomino_64x64.txt" "r-pentomino" 500
```

Move the result to `crates/henad-models/tests/fixtures/` alongside the glider fixture.

---

This document is assisted with Claude Opus 5, with heavy human edits after generation.

## Other engines

Every engine in the cross-engine benchmarks implements this model from the rules above, and commits
its own fixture as `gol_<scenario>_64x64_<engine>.txt`. `scripts/validate_ports.py` produces them:

```bash
uv run --project scripts scripts/validate_ports.py --engines <engine>
```

`consistency_*.rs` then checks Henad against every fixture in the directory, so a port stays checked
with nothing installed but cargo.
