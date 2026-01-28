mod fixtures {
    wasvy::include_wasvy_components!("tests/fixtures");
}

use wasvy::witgen::{self, WitGeneratorSettings};

#[test]
fn include_wasvy_components_preserves_type_path() {
    let settings = WitGeneratorSettings::default();
    let output = witgen::generate_wit(&settings);
    let expected = format!(
        "wasvy:type-path={}::fixtures::components::Health",
        module_path!()
    );
    assert!(output.contains(&expected), "missing type path: {expected}\n{output}");
}
