extern crate bindgen;

use std::env;
use std::error::Error as StdError;
use std::fs::{read_to_string, File};
use std::io::Write;
use std::path::{Path, PathBuf};

use bindgen::callbacks::{DeriveTrait, ImplementsTrait};

#[cfg(feature = "vendored")]
mod vendored;

const ENV_VARS_TRIGGERING_RECOMPILE: [&str; 2] = ["OUT_DIR", "NGX_OBJS"];

/// The feature flags set by the nginx configuration script.
///
/// This list is a subset of NGX_/NGX_HAVE_ macros known to affect the structure layout or module
/// avialiability.
///
/// The flags will be exposed to the buildscripts of _direct_ dependendents of this crate as
/// `DEP_NGINX_FEATURES` environment variable.
/// The list of recognized values will be exported as `DEP_NGINX_FEATURES_CHECK`.
const NGX_CONF_FEATURES: &[&str] = &[
    "compat",
    "debug",
    "have_epollrdhup",
    "have_file_aio",
    "have_kqueue",
    "http_cache",
    "http_dav",
    "http_gzip",
    "http_realip",
    "http_ssi",
    "http_ssl",
    "http_upstream_zone",
    "http_v2",
    "http_v3",
    "http_x_forwarded_for",
    "pcre",
    "pcre2",
    "quic",
    "ssl",
    "stream_ssl",
    "stream_upstream_zone",
    "threads",
];

/// The operating systems supported by the nginx configuration script
///
/// The detected value will be exposed to the buildsrcipts of _direct_ dependents of this crate as
/// `DEP_NGINX_OS` environment variable.
/// The list of recognized values will be exported as `DEP_NGINX_OS_CHECK`.
const NGX_CONF_OS: &[&str] = &[
    "darwin", "freebsd", "gnu_hurd", "hpux", "linux", "solaris", "tru64", "win32",
];

/// Function invoked when `cargo build` is executed.
/// This function will download NGINX and all supporting dependencies, verify their integrity,
/// extract them, execute autoconf `configure` for NGINX, compile NGINX and finally install
/// NGINX in a subdirectory with the project.
fn main() -> Result<(), Box<dyn StdError>> {
    for (name, value) in env::vars() {
        eprintln!("env: {}={}", name, value);
    }

    let nginx_build_dir = match std::env::var("NGX_OBJS") {
        Ok(v) => PathBuf::from(v).canonicalize()?,
        #[cfg(feature = "vendored")]
        Err(_) => vendored::build()?,
        #[cfg(not(feature = "vendored"))]
        Err(_) => panic!("\"nginx-sys/vendored\" feature is disabled and NGX_OBJS is not specified"),
    };
    // Hint cargo to rebuild if any of the these environment variables values change
    // because they will trigger a recompilation of NGINX with different parameters
    for var in ENV_VARS_TRIGGERING_RECOMPILE {
        println!("cargo:rerun-if-env-changed={var}");
    }
    println!("cargo:rerun-if-changed=build/main.rs");
    println!("cargo:rerun-if-changed=build/wrapper.h");
    // Read autoconf generated makefile for NGINX and generate Rust bindings based on its includes
    generate_binding(nginx_build_dir);
    Ok(())
}

#[derive(Debug)]
struct ExternalType<'a>(&'a str, &'a [DeriveTrait]);

impl<'a> ExternalType<'a> {
    pub fn implements(&self, derive_trait: DeriveTrait) -> bool {
        self.1.contains(&derive_trait)
    }
}

#[derive(Debug)]
struct Crate<'a> {
    name: &'a str,
    items: std::collections::BTreeMap<&'a str, ExternalType<'a>>,
}

impl<'a> Crate<'a> {
    pub fn new(name: &'a str) -> Self {
        Self {
            name,
            items: Default::default(),
        }
    }

    pub fn add_type(&mut self, name: &'a str, traits: &'a [DeriveTrait]) {
        self.items.entry(name).or_insert_with(|| ExternalType(name, traits));
    }

    pub fn add_type_copy(&mut self, name: &'a str) {
        self.add_type(name, &[DeriveTrait::Copy])
    }

    pub fn add_type_copy_debug(&mut self, name: &'a str) {
        self.add_type(name, &[DeriveTrait::Copy, DeriveTrait::Debug])
    }

    pub fn type_names(&self) -> Vec<&str> {
        self.items.values().map(|x| x.0).collect()
    }

    pub fn uses(&self) -> String {
        format!("use {}::{{{}}};", self.name, self.type_names().join(","))
    }
}

#[derive(Debug, Default)]
struct NgxExternalTypes<'a>(Vec<Crate<'a>>);

impl NgxExternalTypes<'_> {
    pub fn new() -> Self {
        let mut this = Self::default();

        this.0.push(Crate::new("libc"));
        let libc = this.0.last_mut().unwrap();

        libc.add_type_copy("glob_t");
        libc.add_type_copy("in6_addr");
        libc.add_type_copy("iocb");
        libc.add_type_copy("sem_t");
        libc.add_type_copy("sockaddr_in");
        libc.add_type_copy("sockaddr_in6");
        libc.add_type_copy("stat");
        libc.add_type_copy_debug("DIR");
        libc.add_type_copy_debug("cmsghdr");
        libc.add_type_copy_debug("cpu_set_t");
        libc.add_type_copy_debug("dirent");
        libc.add_type_copy_debug("gid_t");
        libc.add_type_copy_debug("in6_pktinfo");
        libc.add_type_copy_debug("in_addr_t");
        libc.add_type_copy_debug("in_pktinfo");
        libc.add_type_copy_debug("in_port_t");
        libc.add_type_copy_debug("ino_t");
        libc.add_type_copy_debug("iovec");
        libc.add_type_copy_debug("msghdr");
        libc.add_type_copy_debug("off_t");
        libc.add_type_copy_debug("pid_t");
        libc.add_type_copy_debug("pthread_cond_t");
        libc.add_type_copy_debug("pthread_mutex_t");
        libc.add_type_copy_debug("sockaddr");
        libc.add_type_copy_debug("sockaddr_un");
        libc.add_type_copy_debug("socklen_t");
        libc.add_type_copy_debug("time_t");
        libc.add_type_copy_debug("tm");
        libc.add_type_copy_debug("uid_t");

        this.0.push(Crate::new("openssl_sys"));
        let openssl_sys: &'_ mut Crate = this.0.last_mut().unwrap();
        openssl_sys.add_type("SSL", &[]);
        openssl_sys.add_type("SSL_CTX", &[]);
        openssl_sys.add_type("SSL_SESSION", &[]);

        this
    }

    fn find(&self, name: &str) -> Option<(&Crate, &ExternalType)> {
        for c in &self.0[..] {
            for t in c.items.values() {
                if t.0 == name {
                    return Some((c, t));
                }
            }
        }
        None
    }

    fn blocklist(&self) -> String {
        self.0.iter().flat_map(Crate::type_names).collect::<Vec<_>>().join("|")
    }

    fn uses(&self) -> String {
        self.0.iter().map(Crate::uses).collect::<Vec<_>>().join("\n")
    }
}

impl<'a> bindgen::callbacks::ParseCallbacks for NgxExternalTypes<'a> {
    fn blocklisted_type_implements_trait(&self, name: &str, derive_trait: DeriveTrait) -> Option<ImplementsTrait> {
        let parts = name.split_ascii_whitespace().collect::<Vec<_>>();
        let type_name = match &parts[..] {
            ["const", "struct", n] => n,
            ["const", n] => n,
            ["struct", n] => n,
            [n] => n,
            _ => panic!("unhandled blocklisted type: {}", name),
        };

        if self.find(type_name)?.1.implements(derive_trait) {
            return Some(ImplementsTrait::Yes);
        }
        None
    }
}

/// Generates Rust bindings for NGINX
fn generate_binding(nginx_build_dir: PathBuf) {
    let autoconf_makefile_path = nginx_build_dir.join("Makefile");
    let includes = parse_includes_from_makefile(&autoconf_makefile_path);
    let clang_args: Vec<String> = includes
        .iter()
        .map(|path| format!("-I{}", path.to_string_lossy()))
        .collect();

    print_cargo_metadata(&includes).expect("cargo dependency metadata");

    let callbacks = NgxExternalTypes::new();

    let bindings = bindgen::Builder::default()
        .allowlist_function("ngx_.*")
        .allowlist_type("ngx_.*")
        .allowlist_type("bpf_.*")
        .allowlist_type("sig_atomic_t|u_char|u_short")
        .allowlist_var("(NGX|NGINX|ngx|nginx)_.*")
        .blocklist_type(callbacks.blocklist())
        .raw_line(callbacks.uses())
        .generate_comments(false)
        .generate_cstr(true)
        // The input header we would like to generate bindings for.
        .header("build/wrapper.h")
        .clang_args(clang_args)
        .layout_tests(false)
        .parse_callbacks(Box::new(callbacks))
        .generate()
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_dir_env = env::var("OUT_DIR").expect("The required environment variable OUT_DIR was not set");
    let out_path = PathBuf::from(out_dir_env);
    bindings
        .write_to_file(out_path.join("bindings.rs"))
        .expect("Couldn't write bindings!");

    bindings
        .write_to_file(PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap()).join("bindgen.out"))
        .unwrap();
}

/// Reads through the makefile generated by autoconf and finds all of the includes
/// used to compile nginx. This is used to generate the correct bindings for the
/// nginx source code.
fn parse_includes_from_makefile(nginx_autoconf_makefile_path: &PathBuf) -> Vec<PathBuf> {
    fn extract_include_part(line: &str) -> &str {
        line.strip_suffix('\\').map_or(line, |s| s.trim())
    }
    /// Extracts the include path from a line of the autoconf generated makefile.
    fn extract_after_i_flag(line: &str) -> Option<&str> {
        let mut parts = line.split("-I ");
        match parts.next() {
            Some(_) => parts.next().map(extract_include_part),
            None => None,
        }
    }

    let mut includes = vec![];
    let makefile_contents = match read_to_string(nginx_autoconf_makefile_path) {
        Ok(path) => path,
        Err(e) => {
            panic!(
                "Unable to read makefile from path [{}]. Error: {}",
                nginx_autoconf_makefile_path.to_string_lossy(),
                e
            );
        }
    };

    let mut includes_lines = false;
    for line in makefile_contents.lines() {
        if !includes_lines {
            if let Some(stripped) = line.strip_prefix("ALL_INCS") {
                includes_lines = true;
                if let Some(part) = extract_after_i_flag(stripped) {
                    includes.push(part);
                }
                continue;
            }
        }

        if includes_lines {
            if let Some(part) = extract_after_i_flag(line) {
                includes.push(part);
            } else {
                break;
            }
        }
    }

    let makefile_dir = nginx_autoconf_makefile_path
        .parent()
        .expect("makefile path has no parent")
        .parent()
        .expect("objs dir has no parent")
        .to_path_buf()
        .canonicalize()
        .expect("Unable to canonicalize makefile path");

    includes
        .into_iter()
        .map(PathBuf::from)
        .map(|path| {
            if path.is_absolute() {
                path
            } else {
                makefile_dir.join(path)
            }
        })
        .collect()
}

/// Collect info about the nginx configuration and expose it to the dependents via
/// `DEP_NGINX_...` variables.
pub fn print_cargo_metadata<T: AsRef<Path>>(includes: &[T]) -> Result<(), Box<dyn StdError>> {
    // Unquote and merge C string constants
    let unquote_re = regex::Regex::new(r#""(.*?[^\\])"\s*"#).unwrap();
    let unquote = |data: &str| -> String {
        unquote_re
            .captures_iter(data)
            .map(|c| c.get(1).unwrap().as_str())
            .collect::<Vec<_>>()
            .concat()
    };

    let mut ngx_features: Vec<String> = vec![];
    let mut ngx_os = String::new();

    let expanded = expand_definitions(includes)?;
    for line in String::from_utf8(expanded)?.lines() {
        let Some((name, value)) = line.trim().strip_prefix("RUST_CONF_").and_then(|x| x.split_once('=')) else {
            continue;
        };

        let name = name.trim().to_ascii_lowercase();
        let value = value.trim();

        if name == "nginx_build" {
            println!("cargo::metadata=build={}", unquote(value));
        } else if name == "nginx_version" {
            println!("cargo::metadata=version={}", unquote(value));
        } else if name == "nginx_version_number" {
            println!("cargo::metadata=version_number={value}");
        } else if NGX_CONF_OS.contains(&name.as_str()) {
            ngx_os = name;
        } else if NGX_CONF_FEATURES.contains(&name.as_str()) && value != "0" {
            ngx_features.push(name);
        }
    }

    println!(
        "cargo::metadata=include={}",
        // The str conversion is necessary because cargo directives must be valid UTF-8
        env::join_paths(includes.iter().map(|x| x.as_ref()))?
            .to_str()
            .expect("Unicode include paths")
    );

    // A quoted list of all recognized features to be passed to rustc-check-cfg.
    println!("cargo::metadata=features_check=\"{}\"", NGX_CONF_FEATURES.join("\",\""));
    // A list of features enabled in the nginx build we're using
    println!("cargo::metadata=features={}", ngx_features.join(","));

    // A quoted list of all recognized operating systems to be passed to rustc-check-cfg.
    println!("cargo::metadata=os_check=\"{}\"", NGX_CONF_OS.join("\",\""));
    // Current detected operating system
    println!("cargo::metadata=os={ngx_os}");

    Ok(())
}

fn expand_definitions<T: AsRef<Path>>(includes: &[T]) -> Result<Vec<u8>, Box<dyn StdError>> {
    let path = PathBuf::from(env::var("OUT_DIR")?).join("expand.c");
    let mut writer = std::io::BufWriter::new(File::create(&path)?);

    write!(
        writer,
        "
#include <ngx_config.h>
#include <nginx.h>

RUST_CONF_NGINX_BUILD=NGINX_VER_BUILD
RUST_CONF_NGINX_VERSION=NGINX_VER
RUST_CONF_NGINX_VERSION_NUMBER=nginx_version
"
    )?;

    for flag in NGX_CONF_FEATURES.iter().chain(NGX_CONF_OS.iter()) {
        let flag = flag.to_ascii_uppercase();
        write!(
            writer,
            "
#if defined(NGX_{flag})
RUST_CONF_{flag}=NGX_{flag}
#endif"
        )?;
    }

    writer.flush()?;
    drop(writer);

    Ok(cc::Build::new().includes(includes).file(path).try_expand()?)
}
