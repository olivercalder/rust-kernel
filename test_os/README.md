To compile and run, need a few things:

- `rustup toolchain install nightly`
- `rustup override set nightly`
- `cargo install cargo-xbuild`
- `cargo install bootimage`
- `rustup component add rust-src`
- `rustup component add llvm-tools-preview`

Then compile and run the kernel (in QEMU) using `cargo xrun`
