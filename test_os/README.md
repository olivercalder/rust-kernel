To compile and run, need a few things:

- `rustup toolchain install nightly`
  - If this fails, try with the `--force` flag, as there has been a problem with the `rustfmt` package dependency
- `rustup override set nightly`
- `cargo install bootimage`
- `rustup component add rust-src`
- `rustup component add llvm-tools-preview`

Then compile and run the kernel (in QEMU) using `cargo run`

To execute all `test_case` functions, run `cargo test`
