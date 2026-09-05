use serde::Serialize;
use std::fs;
use std::path::Path;

pub fn out<T>(path: &Path, value: &T, pretty: bool) -> Result<(), Box<dyn std::error::Error>>
where
    T: Serialize,
{
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let mut json = if pretty {
        serde_json::to_string_pretty(value)?
    } else {
        serde_json::to_string(value)?
    };

    json.push('\n');

    fs::write(path, json)?;

    Ok(())
}
