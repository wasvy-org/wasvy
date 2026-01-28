use wasvy::witgen::{self, WitComponentInfo, WitGeneratorSettings, WitMethodInfo};

wasvy::witgen::inventory::submit! {
    WitComponentInfo {
        type_path: health_type_path,
        name: "Health",
    }
}

wasvy::witgen::inventory::submit! {
    WitMethodInfo {
        type_path: health_type_path,
        name: "heal",
        arg_names: &["amount"],
        arg_types: &["f32"],
        ret: "()",
        mutable: true,
    }
}

wasvy::witgen::inventory::submit! {
    WitMethodInfo {
        type_path: health_type_path,
        name: "pct",
        arg_names: &[],
        arg_types: &[],
        ret: "f32",
        mutable: false,
    }
}

fn health_type_path() -> &'static str {
    "game::Health"
}

#[test]
fn generates_wit_resources() {
    let settings = WitGeneratorSettings::default();
    let output = witgen::generate_wit(&settings);

    let wasvy_use = "use wasvy:ecs/app.{component}";

    assert!(output.contains("package game:components;"));
    assert!(output.contains("interface components"));
    assert!(output.contains(wasvy_use));
    assert!(output.contains("resource health"));
    assert!(output.contains("wasvy:type-path=game::Health"));
    assert!(output.contains("constructor(component: component)"));
    assert!(output.contains("heal: func(amount: f32)"));
    assert!(output.contains("pct: func() -> f32"));
    assert!(output.contains("world host"));
}
