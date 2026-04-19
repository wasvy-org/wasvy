pub trait Named {
    /// A name for the this value
    ///
    /// Defaults to the shortname.
    ///
    /// ```
    /// use wasvy_cli::named::Named;
    /// struct HelloWorld;
    /// impl Named for HelloWorld {}
    /// assert_eq!(HelloWorld.name(), "HelloWorld");
    /// ```
    fn name(&self) -> &str {
        let full_name = std::any::type_name_of_val(self);
        full_name
            .rsplit_once("::")
            .map(|(_, name)| name)
            .unwrap_or(full_name)
    }
}
