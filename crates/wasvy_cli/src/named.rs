pub trait Named {
    /// A name for the this value
    ///
    /// Defaults to the shortname.
    ///
    /// ```
    /// # use super::Named;
    /// struct HelloWorld;
    /// impl<T> Named for HelloWorld {}
    /// assert!(HelloWorld.name(), "HelloWorld")
    /// ```
    fn name(&self) -> &str {
        let full_name = std::any::type_name_of_val(self);
        full_name
            .rsplit_once("::")
            .map(|(_, name)| name)
            .unwrap_or(full_name)
    }
}
