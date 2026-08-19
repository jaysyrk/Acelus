use std::fs;
use std::io;
use std::path::{Component, Path, PathBuf};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("the archive could not be read")]
    Archive(#[from] zip::result::ZipError),

    #[error("i/o failed at {path}")]
    Io {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error(
        "refusing to extract {entry}: it resolves outside the destination directory, \
         which means the archive is hostile"
    )]
    PathEscape { entry: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Extracted {
    pub files: u64,
    pub bytes: u64,
    pub skipped: u64,
}

pub fn safe_join(base: &Path, entry: &str) -> Option<PathBuf> {
    let mut resolved = base.to_path_buf();

    for component in Path::new(entry).components() {
        match component {
            Component::Normal(part) => resolved.push(part),
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => return None,
        }
    }

    if resolved.starts_with(base) && resolved != base {
        Some(resolved)
    } else {
        None
    }
}

pub fn is_excluded(entry: &str, exclude: &[String]) -> bool {
    exclude
        .iter()
        .any(|prefix| entry.starts_with(prefix.as_str()))
}

pub fn extract_natives(
    archive: &[u8],
    destination: &Path,
    exclude: &[String],
) -> Result<Extracted> {
    let mut zip = zip::ZipArchive::new(io::Cursor::new(archive))?;
    let mut extracted = Extracted::default();

    fs::create_dir_all(destination).map_err(|source| Error::Io {
        path: destination.to_path_buf(),
        source,
    })?;

    for index in 0..zip.len() {
        let mut entry = zip.by_index(index)?;
        let name = entry.name().to_string();

        if name.ends_with('/') {
            continue;
        }

        if is_excluded(&name, exclude) {
            extracted.skipped += 1;
            continue;
        }

        let target = safe_join(destination, &name).ok_or(Error::PathEscape {
            entry: name.clone(),
        })?;

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).map_err(|source| Error::Io {
                path: parent.to_path_buf(),
                source,
            })?;
        }

        let mut file = fs::File::create(&target).map_err(|source| Error::Io {
            path: target.clone(),
            source,
        })?;

        let written = io::copy(&mut entry, &mut file).map_err(|source| Error::Io {
            path: target.clone(),
            source,
        })?;

        extracted.files += 1;
        extracted.bytes += written;
    }

    Ok(extracted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinary_entries_resolve_inside_the_destination() {
        let base = Path::new("/instance/natives");
        assert_eq!(
            safe_join(base, "liblwjgl.so"),
            Some(PathBuf::from("/instance/natives/liblwjgl.so"))
        );
        assert_eq!(
            safe_join(base, "nested/lib.so"),
            Some(PathBuf::from("/instance/natives/nested/lib.so"))
        );
        assert_eq!(
            safe_join(base, "./lib.so"),
            Some(PathBuf::from("/instance/natives/lib.so"))
        );
    }

    #[test]
    fn traversal_entries_are_refused() {
        let base = Path::new("/instance/natives");

        for hostile in [
            "../escaped.so",
            "../../etc/passwd",
            "nested/../../escaped.so",
            "/absolute/escaped.so",
        ] {
            assert_eq!(
                safe_join(base, hostile),
                None,
                "{hostile} should have been refused"
            );
        }
    }

    #[test]
    fn an_entry_resolving_to_the_destination_itself_is_refused() {
        let base = Path::new("/instance/natives");
        assert_eq!(safe_join(base, "."), None);
        assert_eq!(safe_join(base, ""), None);
    }

    #[test]
    fn excluded_prefixes_are_matched_against_the_entry_name() {
        let exclude = vec!["META-INF/".to_string()];

        assert!(is_excluded("META-INF/MANIFEST.MF", &exclude));
        assert!(is_excluded("META-INF/maven/pom.xml", &exclude));
        assert!(!is_excluded("liblwjgl.so", &exclude));
        assert!(!is_excluded("nested/META-INF/x", &exclude));
    }

    #[test]
    fn nothing_is_excluded_without_directives() {
        assert!(!is_excluded("META-INF/MANIFEST.MF", &[]));
    }
}
