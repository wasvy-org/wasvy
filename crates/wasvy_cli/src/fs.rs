use std::{any, fs, path::Path};

use anyhow::{Result, anyhow};

// Implemented for things that can be written to the disk.
pub trait WriteTo {
    // Writes the contents to the disk at a specific path.
    //
    // Currently this trait is implemented for {askama::Template}.
    fn write(&self, path: impl AsRef<Path>) -> Result<()>;
}

impl<T> WriteTo for T
where
    T: askama::Template,
{
    fn write(&self, path: impl AsRef<Path>) -> Result<()> {
        let full_text = self.render()?;
        let (metadata, contents) = full_text
            .split_once("\n")
            .unwrap_or((&full_text, &full_text));
        let (_, file_name) = metadata.split_once("filename:").ok_or_else(|| {
            let name = any::type_name::<T>();
            anyhow!("template {name} is missing metadata comment at the top of the file ex: '# filename: ./example.txt'")
        })?;
        let path = path.as_ref().join(file_name.trim());

        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Avoid updating files that have not changed
        if fs::read_to_string(&path).ok().as_deref() != Some(contents) {
            fs::write(&path, contents)?;
        }

        Ok(())
    }
}
