# Development tools

Run these from the repository root; scripts also locate the repository correctly when invoked by absolute path from another directory.

| Purpose | macOS / Bash | Windows / PowerShell |
| --- | --- | --- |
| All tests + overall score | `bash ./tools/test.sh` | `.\tools\test.ps1` |
| LLVM setup only | `bash ./tools/setup-llvm.sh` | `.\tools\setup-llvm.ps1` |
| Release timing comparison | `bash ./tools/time-release.sh --runs 5` | `.\tools\time-release.ps1 -Runs 5` |

The main `nerv.sh` and `nerv.ps1` launchers remain at the root. LLVM still installs into the root's ignored `llvm/` directory, and compiler builds still use the root `target/` directory. Timing program/executable paths remain relative to the repository root.

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
