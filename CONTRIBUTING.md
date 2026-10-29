# Contributing

Thank you for helping. Bug reports, recorded tracks and pull requests are welcome.

## Licence of contributions

The project is published under the [PolyForm Noncommercial License 1.0.0](LICENSE.md).
The author, Ivan Grunev, keeps the right to release the software under other terms,
including commercial ones.

By submitting a contribution (code, data, documentation or other material) you agree that:

1. it is your own work, or you have the right to submit it;
2. it is licensed to the public under the PolyForm Noncommercial License 1.0.0, like the rest
   of the project;
3. you grant Ivan Grunev a perpetual, worldwide, non-exclusive, royalty-free, irrevocable
   licence to use, copy, modify, distribute, sublicense and sell it, under any terms,
   including commercial ones.

You keep the copyright in your contribution. If you cannot agree to point 3, say so in the
pull request before it is merged.

## Third-party material

Do not commit X-Plane navigation data (CIFP, `earth_fix.dat`, `earth_nav.dat`): it is
copyrighted by Navigraph. Speech models are fetched by the scripts in `tools/` and never
committed. See [NOTICE.md](NOTICE.md).

## Before a pull request

```bash
cargo fmt --all
cargo clippy --workspace --all-features
cargo test --workspace --all-features
```
