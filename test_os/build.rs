/* Details for build.rs found at:
 * https://crates.io/crates/cc
 * https://docs.rs/cc/1.0.58/cc/struct.Build.html
 * https://doc.rust-lang.org/cargo/reference/build-scripts.html
 */
extern crate cc;

fn main() {
    // after running:
    //  gcc -c -o fibonacci.o src/fibonacci.c
    //  ar rcs libfibonacci.a fibonacci.o
    //println!("cargo:rerun-if-changed=src/fibonacci.c");
    //println!("cargo:rustc-link-search=.");    // tried with an absolute path as well, no success
    //println!("cargo:rustc-link-lib=static=libfibonacci.a");   // tried without the static= as well, no success
    
    cc::Build::new()
        .file("src/fibonacci.c")
        .compile("libfibonacci.a");
    // .compile() runs `ar crs` as well
    // The compilation fails because it cannot find symbols from the libraries imported using
    // #include, such as <stdlib.h> and <stdio.h>
}
