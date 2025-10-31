use std::env;
use std::path::PathBuf;
use theta_engine::network::schema;

fn main() {
    if let Err(err) = run() {
        eprintln!("[manifest] error: {err}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let output_path = env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("schemas/component_manifest.json"));

    schema::assert_no_hash_collisions();
    schema::write_manifest_json(&output_path)?;
    println!("[manifest] wrote {} entries to {}", schema::registered_entries().len(), output_path.display());
    Ok(())
}
