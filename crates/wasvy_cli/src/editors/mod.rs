mod generic;
pub use generic::Generic;

use crate::runtime::Config;

impl Config {
    pub fn add_all_editors(&mut self) {
        self.add_editor("atom");
        self.add_editor("clion");
        self.add_editor("emacs");
        self.add_editor("intellij");
        self.add_editor("nvim");
        self.add_editor("pycharm");
        self.add_editor("sublime");
        self.add_editor("vim");
        self.add_editor("vscode");
        self.add_editor("vscodium");
        self.add_editor("webstorm");
        self.add_editor("zed");
        self.add_editor("zeditor");
    }
}
