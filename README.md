<div align="center">
    <img src="./git-assets/nerv.png" height=600>
    <h1>Nerv</h1>

[**Quick Start**](#quick-start) | [**Contributing**](#contributing) | [**License**](#license)

</div>

> [!IMPORTANT]
> Nerv is in early alpha and still in development; breaking bugs and missing features should be expected.

## Quick Start

A basic showcase is in the [**Showcase**](./example/hello.nerv) file.

The [language specification](./docs/language-spec.md) defines the supported alpha rules and includes executable examples and platform-specific instructions for running its conformance suite.

The [small standard library](docs/standard-library.md) provides generic collections, optional/result values, UTF-8 file I/O, directories, process inputs, and time utilities through `import nerv.std`.

The current source tree also provides [JSON, HTTPS, and WebSocket client APIs](docs/networking.md) through `import nerv.json` and `import nerv.net`. These additions require rebuilding the compiler; older release archives do not include them.

## Projects and installation

Download a checksummed alpha compiler archive from [Releases](https://github.com/NPSummers/Nerv-v2/releases), or follow the [installation instructions](docs/installation.md) to build from source. Installed compilers need native build tools for linking applications; Rust and Cargo are needed only to build the compiler.

```sh
nerv new hello
cd hello
nerv check --locked
nerv test --locked
nerv fmt --check
nerv run -- argument
nerv build --release --locked
```

See [project tooling](docs/projects.md) for `Nerv.toml`, local path dependencies, deterministic `Nerv.lock`, code-generation caching, formatting, and process exit codes. The repository launchers also forward project commands from the caller's current directory.

## Contributing

Contributions are welcome, but please make sure you clarify with me before submitting a pull request. You can contact me via my discord `non.menthol.newports`.

## License

Nerv is distributed under the [MIT](./LICENSE) license.
