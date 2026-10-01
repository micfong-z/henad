# Recording the 0.2.0 golden output

Produces the reference files in this folder, which `tests/golden.rs` compares `henad-cli`'s output against byte for byte.

| File | Command |
|---|---|
| `list.txt` | `henad-cli --list` on a machine with a compute adapter |
| `list-without-adapter.txt` | `henad-cli --list` on a machine without one |
| `params/<id>.txt` | `henad-cli <id> --params` |
| `params-json/<id>.json` | `henad-cli <id> --params --json` |

## Why the reference is Henad itself

Scripts parse all three outputs, `scripts/bench_matrix.py` among them, and a script written against 0.2.0 has to keep working.
The tests pin backward compatibility, and the reference is therefore what 0.2.0 printed.
The rule against fixtures made by Henad does not apply, since that rule covers correctness oracles.

`--list` hides the GPU models on a machine without a compute adapter, so it keeps two files.
The test picks the file by whether this machine has an adapter, and compares a GPU model's `--params` only where it has one.
Under `HENAD_REQUIRE_GPU=1` a machine without an adapter fails the test.

## Procedure

Record from the `v0.2.0` tag, never from a changed tree.
A tree that has already changed an output would record its own, and the test would pass against it.

From the repository root, on macOS:

```bash
git worktree add --detach ../henad-0.2.0 v0.2.0
cd ../henad-0.2.0
git log --oneline -1          # 773a7a5 release: bump version to 0.2.0
cargo build --release --locked -p henad-cli
golden=../henad/crates/henad-cli/tests/golden
mkdir -p "$golden/params" "$golden/params-json"
target/release/henad-cli --list > "$golden/list.txt"
sandbox-exec -p '(version 1)(allow default)(deny iokit-open)' \
    target/release/henad-cli --list > "$golden/list-without-adapter.txt"
for id in $(target/release/henad-cli --list | awk 'NR > 1 { print $1 }'); do
    target/release/henad-cli "$id" --params > "$golden/params/$id.txt"
    target/release/henad-cli "$id" --params --json > "$golden/params-json/$id.json"
done
target/release/henad-cli --info
cd - && git worktree remove ../henad-0.2.0
```

`golden` is this folder, seen from the worktree. Adjust it to where the checkout sits.
The sandbox profile denies the process every IOKit connection, and Metal then offers no adapter.
The CLI prints its note that no GPU is available on standard error, and standard output holds the six CPU models.
On Linux, run the same `--list` on a machine with no Vulkan driver for the second file.
`--info` names the machine and the adapter.

## Recorded

Recorded on 2026-10-01 on an Apple M4 Pro (macOS, aarch64, 14 logical CPUs) with its Metal adapter, from a release build at 773a7a5 on rustc 1.97.1.
The run without an adapter printed `note: no GPU available (no suitable GPU adapter found); GPU models disabled` on standard error.
