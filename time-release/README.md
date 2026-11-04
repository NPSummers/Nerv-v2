# Rust loop baseline

Adapted from [bddicken/languages' Rust loops benchmark](https://github.com/bddicken/languages/tree/main/loops/rust/src).

This standalone crate matches `example/heavy.nerv`: 10,000 array elements, 100,000 inner iterations, a divisor of 22, and a random value in `0..10_000`. It uses `i64` arithmetic to match Nerv. Each process picks its own random value, so output values can differ.

Run both release benchmarks from the repo root:

```sh
./tools/time-release.sh --runs 5
```

```powershell
.\tools\time-release.ps1 -Runs 5
```

To run just the Rust baseline:

```sh
cargo run --release --manifest-path time-release/Cargo.toml
```

An optional positional argument overrides the divisor. The default is passed through `black_box` to prevent constant-divisor optimization. Release builds use the upstream benchmark's optimization settings.
