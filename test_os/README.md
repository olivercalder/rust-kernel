To compile and run, need a few things:

- `rustup toolchain install nightly-2020-07-17`, since there is a problem with Symlinks with the newest nightly as of 22 July 2020
- `rustup override set nightly-2020-07-17`
- `cargo install bootimage`
- `rustup component add rust-src`
- `rustup component add llvm-tools-preview`

Then compile and run the kernel (in QEMU) using `cargo run`
