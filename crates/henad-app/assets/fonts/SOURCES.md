# Font sources

The app embeds four fonts.
Each is listed here with its source, its licence and, for the two built here, the procedure that rebuilds it.

| File | Source | Licence |
|---|---|---|
| `Henad Sans Regular.ttf` | IBM Plex Sans 3.201, modified as described below | SIL Open Font License 1.1, in `OFL.txt` |
| `Henad Mono Regular.ttf` | IBM Plex Mono 2.3, modified as described below | SIL Open Font License 1.1, in `OFL.txt` |
| `Material Design Icons.ttf` | `fonts/materialdesignicons-webfont.ttf` of [Templarian/MaterialDesign-Webfont](https://github.com/Templarian/MaterialDesign-Webfont) at tag `v7.2.96`, unmodified | Apache License 2.0, in `LICENSE-APACHE` |
| `Material Symbols Outlined.ttf` | `variablefont/MaterialSymbolsOutlined[FILL,GRAD,opsz,wght].ttf` of [google/material-design-icons](https://github.com/google/material-design-icons) at commit `9365c0b8c619f5888bbf6f9497ca0f1d5e27c5b1`, version 2.667, unmodified | Apache License 2.0, in `LICENSE-APACHE`. Copyright 2020-2023 Google LLC |

The Material Design Icons file has SHA-256 `a58ecb54f45eec1afadbc21314d1f0932cf009e5cbc7f3225d7e4a4e1b71ef6b`, the same bytes as the upstream file at its tag.
The Material Symbols file has SHA-256 `3527004d40bc8706368af0c247876c272a25fd3f17d70e1ba24c84d1c0b5e499`, the same bytes as the upstream file at its commit.

## Henad Sans and Henad Mono

The two text fonts are IBM Plex with three OpenType features made permanent: `ss01` (single-storey a), `ss02` (single-storey g) and `zero` (slashed zero).
egui applies no OpenType features, so the glyphs have to be the defaults.

IBM reserves the font name "Plex" under the SIL Open Font License, and a modified version cannot carry it.
The modified fonts are therefore named Henad Sans and Henad Mono.
IBM's copyright, licence and trademark records stay in each file unchanged, and `OFL.txt` is IBM's licence text with its Reserved Font Name line.
IBM Plex is a trademark of IBM Corp.

The upstream files are Google Fonts' copies, taken at commit `0b58fb370093f9a9f4ff785d94405710b79de67c` of [google/fonts](https://github.com/google/fonts).

| Upstream file | Version | SHA-256 |
|---|---|---|
| `ofl/ibmplexsans/IBMPlexSans[wdth,wght].ttf` | 3.201 | `3b031aa4216174205bd8471f88a49b91f093169e9e87bd5262242bc5967fe2e3` |
| `ofl/ibmplexmono/IBMPlexMono-Regular.ttf` | 2.3 | `6a3412f058c7d8dfd9170c41e85ade48e5156ecb89356110ca57a0a27734af46` |
| `ofl/ibmplexsans/OFL.txt` | | `7e6b2818edbd8f6a01ae80641cc8f16a51080d08fb4e532be3a0b6f74adb07da` |

Plex Sans 3.201 is published as a variable font only.
Its regular instance sits at the default axis values, weight 400 and width 100.

### Procedure

The procedure needs `curl` and [uv](https://docs.astral.sh/uv/), and pins fontTools 4.66.1 and opentype-feature-freezer 1.32.2.
Run it in an empty directory.

```bash
commit=0b58fb370093f9a9f4ff785d94405710b79de67c
base="https://raw.githubusercontent.com/google/fonts/$commit/ofl"
curl -sSfL -o 'IBMPlexSans[wdth,wght].ttf' "$base/ibmplexsans/IBMPlexSans%5Bwdth%2Cwght%5D.ttf"
curl -sSfL -o IBMPlexMono-Regular.ttf "$base/ibmplexmono/IBMPlexMono-Regular.ttf"
curl -sSfL -o OFL.txt "$base/ibmplexsans/OFL.txt"
shasum -a 256 'IBMPlexSans[wdth,wght].ttf' IBMPlexMono-Regular.ttf OFL.txt

# fontTools stamps the head table with this time in place of the clock, so the output hashes repeat.
export SOURCE_DATE_EPOCH=1772556323
fonttools=(uvx --from 'fonttools==4.66.1')
freezer=(uvx --from 'opentype-feature-freezer==1.32.2' --with 'fonttools==4.66.1' pyftfeatfreeze)

# Cut the static regular instance out of the variable Sans.
"${fonttools[@]}" fonttools varLib.instancer 'IBMPlexSans[wdth,wght].ttf' wght=400 wdth=100 \
    -o IBMPlexSans-Regular.ttf

# Freeze the three features into the character map, and rename the family.
"${freezer[@]}" -f ss01,ss02,zero -i -R 'IBM Plex Sans/Henad Sans,IBMPlexSans/HenadSans' \
    IBMPlexSans-Regular.ttf 'Henad Sans Regular.ttf'
"${freezer[@]}" -f ss01,ss02,zero -i -R 'IBM Plex Mono/Henad Mono,IBMPlexMono/HenadMono' \
    IBMPlexMono-Regular.ttf 'Henad Mono Regular.ttf'
```

`-R` leaves the old name in the unique font identifier (name ID 3), and the Sans keeps the variable font's style attributes (the `STAT` table) with their names.
A last step drops `STAT`, renames every remaining record but the copyright (0), trademark (7) and licence (13) records, and removes the names nothing refers to.

```bash
cat > finish.py <<'EOF'
import sys
from fontTools.ttLib import TTFont

path, family = sys.argv[1], sys.argv[2]
upstream = family.replace("Henad", "IBM Plex")
font = TTFont(path)
if "STAT" in font:
    del font["STAT"]
for record in font["name"].names:
    if record.nameID not in (0, 7, 13):
        text = record.toUnicode()
        text = text.replace(upstream, family).replace(upstream.replace(" ", ""), family.replace(" ", ""))
        record.string = text
font["name"].removeUnusedNames(font)
font.save(path)
EOF
"${fonttools[@]}" python finish.py 'Henad Sans Regular.ttf' 'Henad Sans'
"${fonttools[@]}" python finish.py 'Henad Mono Regular.ttf' 'Henad Mono'

# "Plex" is left only in the trademark record.
"${fonttools[@]}" ttx -q -t name -o - 'Henad Sans Regular.ttf' | grep Plex
"${fonttools[@]}" ttx -q -t name -o - 'Henad Mono Regular.ttf' | grep Plex
```

The procedure produces these files.

| File | SHA-256 |
|---|---|
| `Henad Sans Regular.ttf` | `4cdd4289605a370bfc3866f3f9c3aab42e19a27878a5e19c547f7da152535aea` |
| `Henad Mono Regular.ttf` | `768b2fc782b99ccd271aa79709972ec015e10b7d48da781f1c500835416f0473` |

Every character of both fonts draws with the same outline and advance width as in the fonts the app embedded up to Henad 0.2.0, which FontFreeze 1.12.4 froze from the same upstream versions with the same three features.
