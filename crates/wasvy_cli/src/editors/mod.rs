mod generic;
pub use generic::Generic;

use crate::runtime::Config;

impl Config {
    pub fn add_all_editors(&mut self) {
        self.add_editor(Generic::new("atom"));
        self.add_editor(Generic::new("clion"));
        self.add_editor(Generic::new("emacs"));
        self.add_editor(Generic::new("intellij"));
        self.add_editor(Generic::new("neovim"));
        self.add_editor(Generic::new("pycharm"));
        self.add_editor(Generic::new("sublime"));
        self.add_editor(Generic::new("vim"));
        self.add_editor(Generic::new("vscode"));
        self.add_editor(Generic::new("vscodium"));
        self.add_editor(Generic::new("webstorm"));
        self.add_editor(Generic::new("zed"));
        self.add_editor(Generic::new("zeditor"));
    }
}
