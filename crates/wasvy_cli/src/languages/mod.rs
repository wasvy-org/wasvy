mod python;
pub use python::*;

mod rust;
pub use rust::*;

use crate::runtime::Config;

impl Config {
    pub fn add_all_languages(&mut self) {
        self.add_language(Rust::new().unwrap_or_default(), &["rs", "r"]);
        self.add_language(Python, &["py", "p"]);
    }
}
