fn main() {
    println!(
        r"cargo:rustc-link-search=toolchain/arm-unknown-linux-gnueabihf/arm-unknown-linux-gnueabihf/lib/"
    );
    println!(r"cargo:rustc-link-search=build_env/usr/lib/");
}
