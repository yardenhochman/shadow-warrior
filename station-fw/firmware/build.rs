fn main() {
    embuild::espidf::sysenv::output();
    println!("cargo:rerun-if-env-changed=STATION_BAUD");
}
