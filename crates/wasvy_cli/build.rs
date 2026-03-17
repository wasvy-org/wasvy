fn main() {
    println!("cargo:rerun-if-changed=askama.toml");
    println!("cargo:rerun-if-changed=templates");
}
