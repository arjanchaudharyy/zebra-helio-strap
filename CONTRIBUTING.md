# Contributing

Thanks for helping. Issues and pull requests are welcome.

## Run it

```sh
npm install
npm run dev                        # browser preview; open http://localhost:5173/?demo=1 for demo data
npm run tauri dev                  # the real app
npx tauri build --bundles app,dmg  # release build
```

You need Node 20+, Rust (see `rust-toolchain.toml`) and the Xcode command line tools. The strap client in `strap/` needs Python 3.11+.

## Before you open a pull request

- `npx vue-tsc --noEmit` and `npx vitest run` pass.
- In `src-tauri/`: `cargo test --workspace` passes.
- `cd strap && .venv/bin/python -m pytest` passes if you changed the strap client.
- UI changes: add before and after screenshots, using demo data.
- Write plain English. The app is English only.
- Never commit tokens, keys or your own health data.

## Where things are

- `src/v4/`: the Zebra interface (pages in `src/v4/views/`).
- `src-tauri/`: the Rust app, the core library, the MCP server and the CLI.
- `strap/`: the Python Bluetooth client for the Helio Strap.
- `docs/zebra/helio-strap-hardware.md`: what we know about the strap.
