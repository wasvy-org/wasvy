use anyhow::Result;

use crate::dependency::Dependency;

pub struct Remote {
    pub dependencies: Vec<Dependency>,
}

impl Remote {
    pub fn new() -> Result<Remote> {
        todo!()
    }
}
