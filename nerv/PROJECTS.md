# Project loading

The Nerv-written driver resolves `Nerv.toml` projects and local path dependencies before loading and checking source modules. It checks the selected entry and its imports. It does not yet scan every unimported source/test file, run project tests, or generate executables.

```powershell
.\target\nerv\release\nerv_bootstrap.exe --check --manifest-path Nerv.toml --locked
.\target\nerv\release\nerv_bootstrap.exe --ir src/main.nerv --locked
```

`check` is also accepted as a mode. With no file, the driver discovers the nearest manifest above the current directory and selects its application entry, falling back to its library entry. With a file, discovery starts at that file. `--manifest-path` selects an explicit project from any working directory. Standalone files still work without a manifest; `--locked` requires a project.

## Manifest

```toml
[package]
name = "app"
version = "0.1.0"
entry = "src/main.nerv"
lib = "src/lib.nerv"

[dependencies]
helper = { path = "../helper" }
```

`entry` and `lib` have the defaults shown above. Each source path must be relative, end in `.nerv`, and contain no parent traversal. Package names and dependency aliases are ASCII identifiers. `nerv` is reserved; an alias cannot equal the package name. Versions follow stage 0's three numeric unsigned-64-bit components, including its acceptance of leading zeros and a leading plus sign. These package versions do not implement the release pipeline's full SemVer tag grammar.

Only local path dependencies are supported. Each dependency must have a manifest and an existing library entry. All declared dependencies are validated, even when unused. Canonical package roots deduplicate shared dependencies and detect cycles. The graph is limited to 256 packages; manifests to 1 MiB; inline table nesting to eight levels.

The reader implements the string/table TOML forms needed by this schema: comments, dotted fields, quoted keys, ordinary and multiline basic/literal strings, Unicode escapes, inline tables, and dependency table sections. Unknown fields, wrong value types, duplicate keys, closed inline-table extensions, and remote/version dependencies are rejected. It is not a general TOML parser: arrays, numeric values, dates, and arrays of tables are outside the manifest schema. Quoted keys containing dots are explicitly rejected.

## Import resolution

`import helper` loads that alias's library entry. `import helper.extra` loads `extra.nerv` beside the library entry. The package's own name resolves to its library when one exists. Dependencies use their own aliases, so two packages can assign different libraries to the same alias.

Declared package aliases take priority over local files. Other source imports first search beside the importing file and then in its owning package's `src` directory. Standard `nerv.*` imports retain their native/source mappings. Sources outside all package roots fall back to their own directory.

## Locks

The root project's `Nerv.lock` records package names, versions, relative manifest paths, dependency aliases, and SHA-256 checksums of manifest UTF-8 bytes. The hash implementation is written in Nerv. Package and alias ordering is deterministic. Source edits alone do not change the lock; manifest edits do.

Without `--locked`, stale or missing locks are written through a temporary sibling file and renamed into place. With `--locked`, missing/stale locks fail without writing. Lock generation matches stage 0 byte for byte in the Windows regression fixtures, including Unicode/spaced dependency paths. Cross-volume Windows dependencies are rejected because their locks cannot use portable relative paths. Temporary-file creation is not exclusive; concurrent writers to the same project are not a supported workflow yet.

## Verification and remaining work

`tools/check-projects.py` compares 29 fixtures with stage 0, validates checked IR, compares lock bytes/checksums, checks missing/stale locks and refreshes, and runs explicit-manifest checks from another working directory. Separate unit tests cover manifest syntax/schema and SHA-256 standard vectors/UTF-8 encoding.

Native macOS/Linux behavior, exhaustive TOML compatibility, non-UTF-8 manifests, filesystem case/symlink edge cases, and whole-project source discovery remain unverified or unimplemented. The new compiler still emits no machine code. The next backend work is bootstrap C interoperability followed by the selected direct LLVM C API path.
