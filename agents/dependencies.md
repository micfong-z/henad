# Crate dependency rule

The full dependency rule behind the crate graph in `AGENTS.md`. Read it before adding or moving
a dependency between Henad crates.

The rule (decision 2.14 of #48): henad-core depends on nothing, and henad-build on henad-core alone.
Every other normal or build dependency runs from a crate to one drawn above it in the graph in
`AGENTS.md`: henad-explore and henad-models onto henad-compute, the hosts onto henad-explore, henad-compute and, for
`example_models()`, henad-models, and any crate with WGSL or a build stamp onto henad-build.
henad-cli and henad-app take henad-models only behind their default `example-models` feature, and
`cargo tree -p henad-cli --no-default-features -e normal -i henad-models` prints nothing, as does
the same line for henad-app. The facade takes henad-models, henad-app and henad-cli only behind its
features, and the two hosts with `default-features = false`, so
`cargo tree -p henad --features app,cli -e normal -i henad-models` reports that no package matches
`henad-models` and exits 101, since that resolve holds no henad-models. henad-models and
henad-explore take no normal dependency on each other,
and `cargo tree -p henad-explore -e normal -i henad-models` prints nothing. Every dev-dependency
between Henad crates that the normal graph does not hold is named here: today the one from
henad-explore to henad-models, for its tests, the one from henad-models to henad-explore with its
`testing` feature, for the kit over the example models, and the ones from henad-cli and henad-app
to henad-models, for their unit tests without the feature. henad-models also takes henad-compute
again as a dev-dependency, with `runtime-wgsl`, a feature on an edge the normal graph holds.
henad-app and henad-cli take henad-explore with `model-files`. The pair between henad-models and
henad-explore is the one dev-only edge against the dependency direction. Neither normal graph
reaches the other, each crate compiles once, and packaging strips both edges. Neither package's
own tests then build from its tarball. henad-explore reaching
an example model outside its tests needs the maintainer's approval as a new edge. henad-tutorial
depends on the facade and, to build, on henad-build. Its tests take the facade again with
`example-models` and `testing`, and it names no other Henad crate. henad-hot depends on
henad-core and henad-compute. henad-app takes it behind its own `hot` feature, an edge #49 approved,
and the facade behind `hot`, each in its native target table. The facade's `hot` turns on `app` and
henad-app's `hot`. Outside the workspace, henad-dylib depends on the facade alone, at `=X.Y.Z` with
`hot`, and hot-host on the facade, henad-models, henad-dylib and henad-build by path, and under
`check` on its own `generation` crate. No workspace member depends on henad-dylib, as
`scripts/check_packaging.sh` checks. A `--workspace` build would select it as a primary package,
cargo builds a primary package with a static std, and every member linking it would fail with
"cannot satisfy dependencies so `std` only shows up once".
