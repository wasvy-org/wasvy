use std::path::Path;
use wasvy_interface::{BEVY_ECS, BEVY_VERSION, WASVY_ECS};
use wit_parser::Resolve;

#[test]
fn valid() {
    let mut resolve = Resolve::default();
    resolve.push_str("bevy-ecs.wit", BEVY_ECS).unwrap();
    let package = resolve.push_str("wasvy-ecs.wit", WASVY_ECS).unwrap();
    let package = &resolve.packages[package];

    assert_eq!(package.name.namespace, "wasvy");
    assert_eq!(package.name.name, "ecs");
    assert_eq!(
        package.name.version.as_ref().unwrap().to_string(),
        env!("CARGO_PKG_VERSION")
    );
    assert!(package.worlds.contains_key("host"));
    assert!(package.worlds.contains_key("guest"));
}

#[test]
fn bevy_version() {
    let mut resolve = Resolve::default();
    let bevy = resolve.push_str("bevy-ecs.wit", BEVY_ECS).unwrap();
    assert_eq!(
        resolve.packages[bevy]
            .name
            .version
            .as_ref()
            .unwrap()
            .to_string(),
        BEVY_VERSION
    );
}

#[test]
fn examples() {
    let examples = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples");
    assert!(
        check_wit_copies(&examples) > 0,
        "no example WIT copies found"
    );
}

/// Locates wit files in the examples folder and check them against the source of truth. Have they drifted?
fn check_wit_copies(directory: &Path) -> usize {
    let mut checked = 0;
    for entry in std::fs::read_dir(directory).unwrap() {
        let entry = entry.unwrap();
        let file_type = entry.file_type().unwrap();
        let path = entry.path();
        if file_type.is_dir() {
            checked += check_wit_copies(&path);
        } else if file_type.is_file() {
            let declaration = match entry.file_name().to_str() {
                Some("bevy-ecs.wit") => BEVY_ECS,
                Some("wasvy-ecs.wit") => WASVY_ECS,
                _ => continue,
            };
            let contents = std::fs::read_to_string(&path).unwrap();
            assert_eq!(contents, declaration, "{} has drifted", path.display());
            checked += 1;
        }
    }
    checked
}
