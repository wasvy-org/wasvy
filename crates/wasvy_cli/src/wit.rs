use std::fmt;

#[derive(Debug, PartialEq, Eq)]
pub struct Exports {
    pub methods: Vec<Method>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct Method {
    pub name: String,
    pub args: Vec<MethodArg>,
}

#[derive(Debug, PartialEq, Eq)]
pub struct MethodArg {
    pub name: String,
    pub import: SystemImport,
}

#[derive(Debug, PartialEq, Eq, Hash)]
pub enum SystemImport {
    Commands,
    Query,
}

impl fmt::Display for SystemImport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Commands => "commands",
            Self::Query => "query",
        })
    }
}
