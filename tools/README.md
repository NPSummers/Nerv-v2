# Development tools

Run these from the repository root; scripts also locate the repository correctly when invoked by absolute path from another directory.

| Purpose | macOS / Bash | Windows / PowerShell |
| --- | --- | --- |
| All tests + overall score | `bash ./tools/test.sh` | `.\tools\test.ps1` |
| LLVM setup only | `bash ./tools/setup-llvm.sh` | `.\tools\setup-llvm.ps1` |
| Standalone Nerv vs Rust release (1 billion nested iterations) | `bash ./tools/time-release.sh --runs 5 --warmups 1` | `.\tools\time-release.ps1 -Runs 5 -Warmups 1` |
| Runtime memory checks (nightly Miri required) | `bash ./tools/check-memory.sh` | Use `cargo +nightly miri test -p nerv-runtime --no-default-features -- --skip tests::supports_collections` with `MIRIFLAGS=-Zmiri-strict-provenance` on a supported Miri host |

The main `nerv.sh` and `nerv.ps1` launchers remain at the root; they run source files or forward `build ...` to the compiler. LLVM still installs into the root's ignored `llvm/` directory, and compiler builds still use the root `target/` directory. Timing program/executable paths remain relative to the repository root.

To keep LLVM's environment in your current shell, source the setup helper:

```sh
source ./tools/setup-llvm.sh
setup_llvm
```

```powershell
. .\tools\setup-llvm.ps1
```

`test-runner/` is a dependency-free Rust workspace tool. The test scripts bootstrap it with Cargo, then it runs all selected test targets with `--no-fail-fast`, streams their normal output, and aggregates the standard Rust test summaries. Cargo arguments and libtest arguments after `--` are forwarded unchanged. Example:

```sh
bash ./tools/test.sh -p nerv --test diagnostics -- --test-threads=1
```

See [testing](../docs/testing.md) for score calculation, failure handling, and CI scope. A score is a test pass rate, not a measurement of overall compiler correctness.

See the [benchmark methodology](../time-release/README.md) for executable paths, output validation, JSON reports, and optimization controls. Bash uses the standard-library-only `compare-binaries.py`; PowerShell uses its native stopwatch. `--skip-build` / `-SkipBuild` times existing standalone executables without setting up LLVM. To test the benchmark helper without running a billion iterations:

```sh
python3 -m unittest discover -s tools -p 'test_compare_binaries.py'
cargo test --locked --manifest-path time-release/Cargo.toml
```

Project commands (`new`, `run`, `build`, `check`, `test`, `fmt`) are forwarded by the root launchers from the caller's working directory. See [project tooling](../docs/projects.md). `tools/package-release.py` builds a redistributable archive from an already-built release compiler, and `tools/install.ps1` / `tools/install.sh` verify and install a downloaded archive without editing PATH. See [installation](../docs/installation.md).

The native networking/JSON integration suite is `cargo test --locked -p nerv --test networking` after LLVM setup. It uses loopback peers and generated TLS certificates, without Discord credentials.
