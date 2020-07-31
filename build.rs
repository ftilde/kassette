fn main() {
    println!(r"cargo:rustc-link-search=build_env/usr/lib/");
    println!("cargo:rustc-link-lib=static=c");
}
