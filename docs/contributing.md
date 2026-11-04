# Contributing

## Layout

```
spec/        nbp.h, neb_stream.h: the protocol, copied from Instrix-Nebulosa-Firmware
docs/        this site (MkDocs); docs/API.md is the binding contract
tools/       nebulosa_sim.py, 99-nebulosa.rules
python/ c/ cpp/ rust/ odin/
```

## When the firmware protocol changes

1. Copy `common/nbp/nbp.h` and `common/fpga/neb_stream.h` from the firmware
   into `spec/`.
2. Update [API.md](API.md) and every binding (records, commands, constants).
3. Teach the simulator the change and add a test in each language.

## Running everything locally

```sh
pip install -e "./python[numpy,test]" && pytest python/tests
cmake -S c -B c/build && cmake --build c/build && (cd c/build && ctest)
cmake -S cpp -B cpp/build && cmake --build cpp/build && (cd cpp/build && ctest)
(cd rust && cargo test && cargo clippy --all-features --all-targets)
odin test odin/tests -collection:nebulosa=odin
(cd rust && cargo doc --no-deps --all-features) && mkdir -p docs/api && cp -r rust/target/doc docs/api/rust
pip install -r docs/requirements.txt && mkdocs serve      # this site, http://127.0.0.1:8000
```

## CI / CD

- **CI** (`.github/workflows/ci.yml`): every push and pull request runs the
  test suites (Python 3.8 and 3.12; C / C++ on Linux, macOS, Windows; Rust
  on all three plus clippy and docs; Odin build and tests), all against the
  simulator.
- **Docs** (`.github/workflows/docs.yml`): pushes to `main` build this site
  plus the Rust API docs and deploy them to GitHub Pages.
- **Release** (`.github/workflows/release.yml`): pushing a tag `vX.Y.Z`
  builds the Python wheel / sdist, the Rust crate and a C/C++ source archive,
  and attaches them to a GitHub Release. Publishing to PyPI and crates.io is
  off until the repository variables `PUBLISH_PYPI` / `PUBLISH_CRATES` are
  set to `true` (see the workflow for the one-time setup).
