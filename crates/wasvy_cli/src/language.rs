use std::{
    any::{Any, TypeId, type_name_of_val},
    fmt,
    path::Path,
};

use anyhow::Result;

use crate::source::Source;

pub trait Language: Any {
    /// A display name for the language, such as Rust or Python
    fn display(&self) -> &str {
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

    /// Generates a new project directory at the given path
    fn generate(&self, source: &Source) -> Result<()> {
        let _ = source;
        Ok(())
    }

    /// Gets the name from a project directory
    fn name(&self, source: &Source) -> Option<String> {
        let _ = source;
        None
    }
}

impl dyn Language {
    pub fn as_any(&self) -> &dyn Any {
        self
    }

    pub fn id(&self) -> LanguageId {
        LanguageId(self.as_any().type_id(), self.display().to_string())
    }
}

#[derive(Eq, PartialEq, Clone, Hash)]
pub struct LanguageId(TypeId, String);

impl fmt::Debug for LanguageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("LanguageId").field(&self.0).finish()
    }
}

impl LanguageId {
    pub fn new<L>(language: &L) -> Self
    where
        L: Language,
    {
        Self(language.type_id(), language.display().to_string())
    }
}

#[cfg(test)]
mod tests {
    use crate::language::Language;

    struct TestLang;

    impl Language for TestLang {}

    #[test]
    fn default_name() {
        assert_eq!(TestLang.display(), "TestLang")
    }
}
