use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    let schema_files = ["schemas/network.fbs"];
    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let out_dir = PathBuf::from(env::var("OUT_DIR").expect("OUT_DIR"));

    println!("cargo::rustc-check-cfg=cfg(has_generated_network_schema)");

    for schema in &schema_files {
        println!("cargo:rerun-if-changed={schema}");
    }

    let flatc_path = which_flatc().expect("flatc compiler not found; install from https://flatbuffers.dev or set FLATC env var");

    let rust_out = out_dir.join("flatbuffers");
    let cpp_out = out_dir.join("flatbuffers_cpp");
    let ts_out = out_dir.join("flatbuffers_ts");

    create_dir(&rust_out);
    create_dir(&cpp_out);
    create_dir(&ts_out);

    for schema in &schema_files {
    compile_schema(&flatc_path, &manifest_dir, schema, &["--rust"], &rust_out);
    compile_schema(&flatc_path, &manifest_dir, schema, &["--cpp"], &cpp_out);
    compile_schema(&flatc_path, &manifest_dir, schema, &["--ts"], &ts_out);
    }

    // Copy TypeScript and C++ outputs into target sidecar directories for developer consumption.
    let artifacts_dir = manifest_dir.join("target").join("flatbuffers");
    let artifacts_cpp = artifacts_dir.join("cpp");
    let artifacts_ts = artifacts_dir.join("ts");

    copy_dir(&cpp_out, &artifacts_cpp);
    copy_dir(&ts_out, &artifacts_ts);

    println!("cargo:rustc-cfg=has_generated_network_schema");
}

fn which_flatc() -> Option<PathBuf> {
    if let Ok(path) = env::var("FLATC") {
        return Some(PathBuf::from(path));
    }

    if let Ok(path) = which::which("flatc") {
        return Some(path);
    }

    None
}

fn compile_schema(flatc_path: &Path, manifest_dir: &Path, schema: &str, language: &[&str], out_dir: &Path) {
    let status = Command::new(flatc_path)
        .current_dir(manifest_dir)
        .args(language)
        .arg("--filename-suffix")
        .arg("_generated")
        .arg("-o")
        .arg(out_dir)
        .arg(schema)
        .status()
        .expect("failed to execute flatc");

    if !status.success() {
        panic!("flatc failed for {}", schema);
    }
}

fn create_dir(path: &Path) {
    fs::create_dir_all(path).expect("failed to create directory");
}

fn copy_dir(src: &Path, dst: &Path) {
    if !dst.exists() {
        fs::create_dir_all(dst).expect("failed to create artifacts directory");
    }

    for entry in fs::read_dir(src).expect("failed to read generated directory") {
        let entry = entry.expect("failed to read entry");
        let file_type = entry.file_type().expect("failed to read file type");
        if file_type.is_file() {
            let filename = entry.file_name();
            let destination = dst.join(filename);
            fs::copy(entry.path(), destination).expect("failed to copy artifact");
        }
    }
}
