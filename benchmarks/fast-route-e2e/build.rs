use std::env;
use std::path::PathBuf;
use std::process::Command;

fn run(mut cmd: Command, label: &str) {
    let status = cmd.status().unwrap_or_else(|err| panic!("{label}: {err}"));
    assert!(status.success(), "{label} failed with {status}");
}

fn main() {
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("OUT_DIR"));
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"))
        .join("../..");
    let route = root.join("experiments/c-fast-route/alpencat_fast_route.c");
    let kernel = root.join("experiments/c-fast-route/native_kernel.c");
    let include = root.join("experiments/c-fast-route");

    println!("cargo:rerun-if-changed={}", route.display());
    println!("cargo:rerun-if-changed={}", kernel.display());
    println!(
        "cargo:rerun-if-changed={}",
        include.join("alpencat_fast_route.h").display()
    );
    println!(
        "cargo:rerun-if-changed={}",
        include.join("native_kernel.h").display()
    );

    let cc = env::var("CC").unwrap_or_else(|_| "cc".to_owned());
    let ar = env::var("AR").unwrap_or_else(|_| "ar".to_owned());
    let route_o = out.join("alpencat_fast_route.o");
    let kernel_o = out.join("native_kernel.o");
    let archive = out.join("libalpencat_fast_route_e2e.a");

    let mut c = Command::new(&cc);
    c.arg("-O3")
        .arg("-std=c11")
        .arg("-I")
        .arg(&include)
        .arg("-c")
        .arg(&route)
        .arg("-o")
        .arg(&route_o);
    run(c, "compile FastRoute");

    let mut k = Command::new(&cc);
    k.arg("-O3")
        .arg("-std=c11")
        .arg("-I")
        .arg(&include)
        .arg("-c")
        .arg(&kernel)
        .arg("-o")
        .arg(&kernel_o);
    run(k, "compile native kernel");

    let mut a = Command::new(&ar);
    a.arg("rcs")
        .arg(&archive)
        .arg(&route_o)
        .arg(&kernel_o);
    run(a, "archive C benchmark objects");

    println!("cargo:rustc-link-search=native={}", out.display());
    println!("cargo:rustc-link-lib=static=alpencat_fast_route_e2e");
}
