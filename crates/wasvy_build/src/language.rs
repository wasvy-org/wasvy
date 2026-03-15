use std::{
    any::{Any, type_name_of_val},
    path::Path,
};

pub trait Language: Any {
    /// A name for the language, such as Rust or Python
    fn name(&self) -> &str {
        let full_name = type_name_of_val(self);
        full_name
            .rsplit_once("::")
            .map(|(_, name)| name)
            .unwrap_or(full_name)
    }

    /// Given a root path on the filesystem, determines whether it is a mod
    /// source. The [Source] returned gives access to apis
    fn identify(&self, root: &Path) -> bool {
        let _ = root;
        false
    }

    /// Generates a directoryin the given path
    fn generate(&self, root: &Path) {
        let _ = root;
    }
}

impl dyn Language {
    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use crate::language::Language;

    struct TestLang;

    impl Language for TestLang {}

    #[test]
    fn default_name() {
        assert_eq!(TestLang.name(), "TestLang")
    }
}
