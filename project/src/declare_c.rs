use std::ffi::c_char;

#[link(name = "server_about_me")] extern "C" {
        pub fn get_about_me() -> *const c_char;
}
#[link(name = "server_about")] extern "C" {
        pub fn get_about() -> *const c_char;
}
#[link(name = "server")] extern "C" {
        pub fn get() -> *const c_char;
}
