mod python;
pub use python::Python;

mod rust;
pub use rust::Rust;

use crate::runtime::Config;

impl Config {
    pub fn add_all_languages(&mut self) {
        self.add_language(Rust::new().unwrap_or_default());
        self.add_language(Python);
    }
}
