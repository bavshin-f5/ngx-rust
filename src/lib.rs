//! Bindings to NGINX
//!
//! This project provides Rust SDK interfaces to the [NGINX](https://nginx.com) proxy allowing the creation of NGINX
//! dynamic modules completely in Rust.
//!
//! ## Build
//!
//! NGINX modules can be built against a particular version of NGINX. The following environment variables can be used
//! to specify a particular version of NGINX or an NGINX dependency:
//!
//! * `ZLIB_VERSION` (default 1.3.1) - zlib version
//! * `PCRE2_VERSION` (default 10.42 for NGINX 1.22.0 and later, or 8.45 for earlier) - PCRE1 or PCRE2 version
//! * `OPENSSL_VERSION` (default 3.2.1 for NGINX 1.22.0 and later, or 1.1.1w for earlier) - OpenSSL version
//! * `NGX_VERSION` (default 1.26.1) - NGINX OSS version
//! * `NGX_DEBUG` (default to false) -  if set to true, then will compile NGINX `--with-debug` option
//!
//! For example, this is how you would compile the [examples](https://github.com/nginx/ngx-rust/tree/master/examples) using a specific version of NGINX and enabling
//! debugging:
//! ```sh
//! NGX_DEBUG=true NGX_VERSION=1.23.0 cargo build --package=examples --examples --release
//! ```
//!
//! To build Linux-only modules, use the "linux" feature:
//! ```sh
//! cargo build --package=examples --examples --features=linux --release
//! ```
//!
//! After compilation, the modules can be found in the path `target/release/examples/` ( with the `.so` file extension for
//! Linux or `.dylib` for MacOS).
//!
//! Additionally, the folder  `.cache/nginx/{NGX_VERSION}/{TARGET}` (`{TARGET}` means rustc's target triple string)
//! will contain the compiled version of NGINX used to build the SDK.
//! You can start NGINX directly from this directory if you want to test the module or add it to `$PATH`
//! ```not_rust
//! $ export NGX_VERSION=1.23.3
//! $ cargo build --package=examples --examples --features=linux --release
//! $ export PATH=$PATH:$PWD/.cache/nginx/$NGX_VERSION/x86_64-unknown-linux-gnu/sbin
//! $ nginx -V
//! $ ls -la ./target/release/examples/
//! # now you can use dynamic modules with the NGINX
//! ```
//!
//! The following environment variables can be used to change locations of cache directory and NGINX directory:
//!
//! * `CACHE_DIR` (default `[nginx-sys's root directory]/.cache`) - the directory containing cache files, means PGP keys, tarballs, PGP signatures, and unpacked source codes. It also contains compiled NGINX in default configuration.
//! * `NGINX_INSTALL_ROOT_DIR` (default `{CACHE_DIR}/nginx`) - the directory containing the series of compiled NGINX in its subdirectories
//! * `NGINX_INSTALL_DIR` (default `{NGINX_INSTALL_BASE_DIR}/{NGX_VERSION}/{TARGET}`) - the directory containing the NGINX compiled in the build
//!
//! ### Mac OS dependencies
//!
//! In order to use the optional GNU make build process on MacOS, you will need to install additional tools. This can be
//! done via [homebrew](https://brew.sh/) with the following command:
//! ```sh
//! brew install make openssl grep
//! ```
//!
//! Additionally, you may need to set up LLVM and clang. Typically, this is done as follows:
//!
//! ```sh
//! # make sure xcode tools are installed
//! xcode-select --install
//! # instal llvm
//! brew install --with-toolchain llvm
//! ```
//!
//! ### Linux dependencies
//!
//! See the [Dockerfile] for dependencies as an example of required packages on Debian Linux.
//!
//! [Dockerfile]: https://github.com/nginxinc/ngx-rust/blob/master/Dockerfile
//!
//! ### Build with external NGINX source tree
//!
//! If you require a customized NGINX configuration, you can build a module against an existing pre-configured source tree.
//! To do that, you need to set the `NGX_OBJS` variable to an _absolute_ path of the NGINX build directory (`--builddir`, defaults to the `objs` in the source directory).
//! Only the `./configure` step of the NGINX build is mandatory because bindings don't depend on any of the artifacts generated by `make`.
//!
//! ```sh
//! NGX_OBJS=$PWD/../nginx/objs cargo build --package=examples --examples
//! ```
//!
//! Furthermore, this approach can be leveraged to build a module as a part of the NGINX build process by adding the `--add-module`/`--add-dynamic-module` options to the configure script.
//! See the following example integration scripts: [`examples/config`] and [`examples/config.make`].
//!
//! [`examples/config`]: https://github.com/nginxinc/ngx-rust/blob/master/examples/config
//! [`examples/config.make`]: https://github.com/nginxinc/ngx-rust/blob/master/examples/config.make

#![warn(missing_docs)]
// support both std and no_std
#![no_std]
#[cfg(all(not(feature = "std"), feature = "alloc"))]
extern crate alloc;
#[cfg(feature = "std")]
extern crate std;

/// The core module.
///
/// This module provides fundamental utilities needed to interface with many NGINX primitives.
/// String conversions, the pool (memory interface) object, and buffer APIs are covered here. These
/// utilities will generally align with the NGINX 'core' files and APIs.
pub mod core;

/// The ffi module.
///
/// This module provides scoped FFI bindings for NGINX symbols.
pub mod ffi;

/// The http module.
///
/// This modules provides wrappers and utilities to NGINX http APIs, such as requests,
/// configuration access, and statuses.
pub mod http;

/// The log module.
///
/// This module provides an interface into the NGINX logger framework.
pub mod log;

/// Define modules exported by this library.
///
/// These are normally generated by the Nginx module system, but need to be
/// defined when building modules outside of it.
#[macro_export]
macro_rules! ngx_modules {
    ($( $mod:ident ),+) => {
        #[no_mangle]
        #[allow(non_upper_case_globals)]
        pub static mut ngx_modules: [*const $crate::ffi::ngx_module_t; $crate::count!($( $mod, )+) + 1] = [
            $( unsafe { &$mod } as *const $crate::ffi::ngx_module_t, )+
            ::core::ptr::null()
        ];

        #[no_mangle]
        #[allow(non_upper_case_globals)]
        pub static mut ngx_module_names: [*const ::core::ffi::c_char; $crate::count!($( $mod, )+) + 1] = [
            $( concat!(stringify!($mod), "\0").as_ptr() as *const ::core::ffi::c_char, )+
            ::core::ptr::null()
        ];

        #[no_mangle]
        #[allow(non_upper_case_globals)]
        pub static mut ngx_module_order: [*const ::core::ffi::c_char; 1] = [
            ::core::ptr::null()
        ];
    };
}

/// Count number of arguments
#[macro_export]
macro_rules! count {
    () => { 0usize };
    ($x:tt, $( $xs:tt ),*) => { 1usize + $crate::count!($( $xs, )*) };
}
