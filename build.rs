use flate2::read::GzDecoder;
use std::{env, fs, path::{Path, PathBuf}};
use tar::Archive;
use which::which;

struct Library(&'static str, &'static str);

fn link_static() -> bool {
    cfg!(feature = "static-link")
}

fn lib_kind() -> &'static str {
    if link_static() { "static" } else { "dylib" }
}

fn build_zlib() -> bool {
    !cfg!(feature = "nozlib")
}

fn build_assimp() -> bool {
    cfg!(feature = "build-assimp")
}

fn compiler_flags() -> Vec<&'static str> {
    let mut f = Vec::new();

    if env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default() == "msvc" {
        f.push("/EHsc");

        if which("ninja").is_ok() {
            env::set_var("CMAKE_GENERATOR", "Ninja");
        }
    }

    f
}

fn lib_names() -> Vec<Library> {
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();

    let mut v = vec![Library("assimp", lib_kind())];

    if build_assimp() && build_zlib() {
        v.push(Library("zlibstatic", "static"));
    } else if os == "windows" {
        v.push(Library("zlibstatic", "dylib"));
    } else {
        v.push(Library("z", "dylib"));
    }

    if os == "linux" {
        v.push(Library("stdc++", "dylib"));
    } else if os == "macos" {
        v.push(Library("c++", "dylib"));
    }
    v
}

fn add_search_paths(dst: &Path) {
    println!("cargo:rustc-link-search=native={}", dst.join("lib").display());
    println!("cargo:rustc-link-search=native={}", dst.join("lib64").display());
    println!("cargo:rustc-link-search=native={}", dst.join("bin").display());
}

trait CMakeFlagsExt {
    fn cflags(&mut self, flags: &[&'static str]) -> &mut Self;
}

impl CMakeFlagsExt for cmake::Config {
    fn cflags(&mut self, flags: &[&'static str]) -> &mut Self {
        for f in flags {
            self.cflag(f).cxxflag(f);
        }
        self
    }
}

fn build_from_source() {
    let dst = cmake::Config::new("assimp")
        .profile("Release")
        .static_crt(true)
        .define("BUILD_SHARED_LIBS", if link_static() { "OFF" } else { "ON" })
        .define("ASSIMP_BUILD_ASSIMP_TOOLS", "OFF")
        .define("ASSIMP_BUILD_TESTS", "OFF")
        .define("ASSIMP_BUILD_ZLIB", if build_zlib() { "ON" } else { "OFF" })
        .define("ASSIMP_WARNINGS_AS_ERRORS", "OFF")
        .define("LIBRARY_SUFFIX", "")
        .cflags(&compiler_flags())
        .build();

    add_search_paths(&dst);
}

fn link_from_package() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let target = env::var("TARGET").unwrap();
    let version = env::var("CARGO_PKG_VERSION").unwrap();
    let archive = format!("russimp-{version}-{target}-{kind}.tar.gz", kind = lib_kind());
    let package_dir = env::var("RUSSIMP_PACKAGE_DIR").map(PathBuf::from).unwrap_or_else(|_| out_dir.clone());
    let archive_path = package_dir.join(&archive);

    if fs::File::open(&archive_path).is_err() {
        let url = format!("https://github.com/Kek5chen/russimp-sys-ng/releases/download/v{version}/{archive}");
        let bytes = reqwest::blocking::get(url).unwrap().bytes().unwrap();
        fs::write(&archive_path, bytes).unwrap();
    }

    let dst = out_dir.join(lib_kind());

    Archive::new(GzDecoder::new(fs::File::open(&archive_path).unwrap()))
        .unpack(&dst)
        .unwrap();

    add_search_paths(&dst);
}

fn ensure_config_header() -> bool {
    let path = Path::new("assimp/include/assimp/config.h");
    if path.exists() { false } else { fs::write(path, "").unwrap(); true }
}

fn generate_bindings(include_dir: &Path) {
    bindgen::builder()
        .header("wrapper.h")
        .clang_arg(format!("-I{}", include_dir.display()))
        .clang_arg("-Iassimp/include")
        .parse_callbacks(Box::new(bindgen::CargoCallbacks::new()))
        .allowlist_type("ai.*")
        .allowlist_function("ai.*")
        .allowlist_var("ai.*")
        .allowlist_var("AI_.*")
        .derive_partialeq(true)
        .derive_eq(true)
        .derive_hash(true)
        .derive_debug(true)
        .generate()
        .unwrap()
        .write_to_file(PathBuf::from(env::var("OUT_DIR").unwrap()).join("bindings.rs"))
        .unwrap();
}

fn main() {
    let out_dir = PathBuf::from(env::var("OUT_DIR").unwrap());
    let os = env::var("CARGO_CFG_TARGET_OS").unwrap();
    if os == "macos" {
        if env::var("CARGO_CFG_TARGET_ARCH").unwrap() == "aarch64" {
            println!("cargo:rustc-link-search=native=/opt/homebrew/lib/");
        } else {
            println!("cargo:rustc-link-search=native=/opt/brew/lib/");
        }
    }
    if build_assimp() {
        build_from_source();
    } else if cfg!(feature = "prebuilt") {
        link_from_package();
    }
    let cleanup = ensure_config_header();
    generate_bindings(&out_dir.join(lib_kind()).join("include"));

    built::write_built_file().unwrap();

    if cleanup { let _ = fs::remove_file("assimp/include/assimp/config.h"); }

    for Library(n, k) in lib_names() {
        println!("cargo:rustc-link-lib={k}={n}");
    }
}

