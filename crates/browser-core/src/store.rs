//! Reading and writing JSON state files without leaving half-written files behind.

use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use serde::Serialize;
use serde::de::DeserializeOwned;

const TEMP_SUFFIX: &str = "tmp";
const BROKEN_SUFFIX: &str = "broken";

/// Reads `path` as JSON. A missing file is `Ok(None)`. A file that cannot be
/// parsed is renamed to `<name>.broken`, so the next save does not overwrite
/// it, and reported as an error.
pub fn load_json<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error).with_context(|| format!("reading {}", path.display())),
    };
    match serde_json::from_str(&text) {
        Ok(value) => Ok(Some(value)),
        Err(error) => {
            let broken = sibling(path, BROKEN_SUFFIX);
            fs::rename(path, &broken).ok();
            Err(error).with_context(|| {
                format!("parsing {} (kept as {})", path.display(), broken.display())
            })
        }
    }
}

/// Writes `value` as JSON to `path`, creating its folder. The file is
/// written next to `path` first and then renamed over it.
pub fn save_json<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    if let Some(folder) = path.parent() {
        fs::create_dir_all(folder).with_context(|| format!("creating {}", folder.display()))?;
    }
    let temp = sibling(path, TEMP_SUFFIX);
    let text = serde_json::to_string_pretty(value)?;
    fs::write(&temp, text).with_context(|| format!("writing {}", temp.display()))?;
    fs::rename(&temp, path).with_context(|| format!("replacing {}", path.display()))
}

fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(suffix);
    path.with_file_name(name)
}

#[cfg(test)]
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("perseu-test-{name}-{}", std::process::id()));
    fs::remove_dir_all(&dir).ok();
    dir
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_reports_missing_files() {
        let path = temp_dir("store-roundtrip")
            .join("nested")
            .join("state.json");
        assert_eq!(load_json::<Vec<u32>>(&path).unwrap(), None);
        save_json(&path, &vec![1, 2, 3]).unwrap();
        save_json(&path, &vec![4]).unwrap();
        assert_eq!(load_json::<Vec<u32>>(&path).unwrap(), Some(vec![4]));
        assert!(!sibling(&path, TEMP_SUFFIX).exists());
    }

    #[test]
    fn broken_files_are_set_aside() {
        let dir = temp_dir("store-broken");
        let path = dir.join("state.json");
        fs::create_dir_all(&dir).unwrap();
        fs::write(&path, "{ not json").unwrap();
        assert!(load_json::<Vec<u32>>(&path).is_err());
        assert!(!path.exists());
        assert!(sibling(&path, BROKEN_SUFFIX).exists());
    }
}
