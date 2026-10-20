# Documentation site

`docs/` is the published documentation site, built by [Zensical](https://zensical.org) from
`zensical.toml` at the repository root. Zensical reads MkDocs Material's configuration format and
Material's Markdown dialect, so admonitions are `!!! note`, not GitHub's `> [!NOTE]`, which renders
as a literal blockquote. Python dependencies are pinned in `pyproject.toml` and `uv.lock`, and CI
builds the site on every pull request and deploys it to GitHub Pages from `master`.

The site's look is CSS over Zensical's modern variant, with no template override and no script.
`docs/stylesheets/mcs.css` lists the MCS entries as `--mcs-<hue>-<step>`, equal to `ui/mcs.rs`, and
`docs/stylesheets/henad.css` names them by role per scheme (`--hd-*`) and points Zensical's `--md-*`
variables at the roles. A new style takes a role rather than a hex value, and corners are square
everywhere. Code is set in subsets of Iosevka Micfong under `docs/assets/fonts`, and `theme.font =
false` keeps Google Fonts out. A fence in a new language needs a label line in `henad.css`.

Anything under `docs/` is published, the session records included. Those sit under
`developing/agent-record/` behind a landing page that tells readers what they are. Notes written for
nobody but the maintainer still do not belong anywhere under `docs/`.

Code examples are included from real workspace files rather than retyped, so an example cannot
drift from the code it documents. The include reads

```md
--8<-- "crates/henad-models/src/game_of_life.rs:step_cell"
```

and the named region is marked in the source with a pair of comments:

```rust
// --8<-- [start:step_cell]
// --8<-- [end:step_cell]
```

Those two lines are a build directive rather than a comment, and are the one exception to the
comment rules in `AGENTS.md`. Do not include by line range, which is also supported and drifts silently.
A whole-file include strips the markers, so a page can show a file whole and in parts.

The template's regions take each file's comment form, `# --8<-- [start:profile]` in TOML and
shell and `<!-- --8<-- [start:fetch] -->` in HTML and Markdown, so a user's copy stays readable.
Its Rust files carry no markers and are included whole. `crates/henad/examples/complete.rs` carries
regions for `guide/library.md`, and its README copy leaves the marker lines out.
`crates/henad/tests/facade_paths.rs` carries the page's two other regions. Every page that
includes template or tutorial code opens with an `!!! info "Henad 0.3"` admonition naming the
release it describes, since the site deploys from `master` and can run ahead of crates.io. An
unused region is deleted with its include.
