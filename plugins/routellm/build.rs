fn main() {
    println!("cargo:rerun-if-env-changed=BAR_RELEASE_VERSION");
}
