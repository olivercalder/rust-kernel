To compile and run, few things need to be set up:

- `rustup toolchain install nightly`
  - If this fails, try with the `--force` flag, as there has been a problem with the `rustfmt` package dependency
- `cargo install bootimage`
  - This must be run using the standard toolchain -- ie. not in a directory where the toolchain is set to nightly
- `rustup component add rust-src`
- `rustup component add llvm-tools-preview`

Then compile and run the kernel (in QEMU) using `cargo run`

To execute all `#[test_case]` functions, run `cargo test`

If compilation fails for some reason seemingly related to the nightly rust compiler, roll back to an earlier nightly version of the format `nightly-YYYY-MM-DD`:

- `rustup toolchain install nightly-YYYY-MM-DD`
  - As above, if this fails, try with `--force` flag
- `rustup override set nightly-YYYY-MM-DD`

**A recent version that worked: `nightly-2020-08-02`**

**Note:** The date indicates the date of archive, not release date of the nightly build. Thus, `nightly-2020-08-02` installs the version with build date 2020-08-01.
