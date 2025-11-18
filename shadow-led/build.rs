fn main() {
    embuild::espidf::sysenv::output();

    // Copy partitions.csv to the ESP-IDF build output directory
    let out_dir = std::env::var("OUT_DIR").unwrap();
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let partition_src = format!("{}/partitions.csv", manifest_dir);
    let partition_dst = format!("{}/partitions.csv", out_dir);

    if std::path::Path::new(&partition_src).exists() {
        std::fs::copy(&partition_src, &partition_dst)
            .expect("Failed to copy partitions.csv to build directory");
        println!("cargo:rerun-if-changed=partitions.csv");
        println!("cargo:warning=Copied partitions.csv to {}", partition_dst);
    }
}
