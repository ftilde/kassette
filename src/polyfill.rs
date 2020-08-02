// These appear to be required, but not actually used (in our use case) in libasound. If we don't
// define them, the linker will complain.

#[no_mangle]
pub unsafe extern "C" fn __sync_synchronize() {
    log!("Unimplemented: sync_synchronize");
    std::process::exit(123);
}

#[no_mangle]
pub unsafe extern "C" fn __sync_sub_and_fetch_4() {
    log!("Unimplemented: sync_sub_and_fetch");
    std::process::exit(123);
}

#[no_mangle]
pub unsafe extern "C" fn __sync_add_and_fetch_4() {
    log!("Unimplemented: sync_add_and_fetch");
    std::process::exit(123);
}
