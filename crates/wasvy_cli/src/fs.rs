use std::{any, fs, path::Path};

use anyhow::{Context, Result, anyhow};

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

        write(&path, contents)?;

        Ok(())
    }
}

// Avoid updating files that have not changed
pub fn write(path: impl AsRef<Path>, contents: impl AsRef<[u8]>) -> Result<()> {
    let path = path.as_ref();

    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }

    if fs::read(path).ok().as_deref() != Some(contents.as_ref()) {
        fs::write(path, contents).with_context(|| format!("path = {path:?}"))?;
    }

    Ok(())
}
