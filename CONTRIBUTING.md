# Contributing to Synkopa

Thanks for helping. Pull requests are welcome.

## Opening a pull request

1. Fork the repo and branch from `main`.
2. Keep one change per PR. Small PRs get merged faster.
3. For a new feature or a larger change, open an issue first so we can agree on the direction.
4. Describe what changed and how you checked it. For UI changes, add a screenshot — or
   better, a short screen capture (GIF) of the new flow; it is what a reviewer plays first.
5. Make sure the checks below pass.

An automated review from CodeRabbit appears on your PR. Treat it as suggestions; the maintainer
decides what gets merged. Comment `@coderabbitai review` to re-run it after you push changes.

## Building and testing

See [Build from source](README.md#build-from-source) for prerequisites. Before you push, run:

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all --check
pnpm --dir ui test
pnpm --dir ui build
```

Tip: a one-off full run does not benefit from incremental compilation — `CARGO_INCREMENTAL=0 cargo test --workspace` is faster and keeps `target/` tens of GB smaller.

[`docs/architecture.md`](docs/architecture.md) explains how the engine, UI and CLI fit together.
[`AGENTS.md`](AGENTS.md) holds the project's quality bar and applies to AI coding agents too.

## License

Synkopa is licensed under [AGPL-3.0-only](LICENSE). By opening a pull request, you agree that your
contribution is licensed under the same terms.
