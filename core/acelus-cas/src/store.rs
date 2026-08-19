use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::{io_error, Error, Hash, Result, HASH_HEX_LEN};

const OBJECTS_DIR: &str = "objects";
const ALIASES_DIR: &str = "aliases";
const TMP_DIR: &str = "tmp";
const SHARD_LEN: usize = 2;

static TMP_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Materialize {
    Shared,
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Reflink,
    Hardlink,
    Copy,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct GcStats {
    pub removed_objects: u64,
    pub removed_bytes: u64,
    pub retained_objects: u64,
    pub retained_bytes: u64,
}

#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Store {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if root.exists() && !root.is_dir() {
            return Err(Error::NotADirectory { path: root });
        }
        for dir in [
            root.join(OBJECTS_DIR),
            root.join(ALIASES_DIR),
            root.join(TMP_DIR),
        ] {
            fs::create_dir_all(&dir).map_err(|e| io_error(&dir, e))?;
        }
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn object_path(&self, hash: &Hash) -> PathBuf {
        let hex = hash.to_hex();
        let (shard, rest) = hex.split_at(SHARD_LEN);
        self.root.join(OBJECTS_DIR).join(shard).join(rest)
    }

    pub fn contains(&self, hash: &Hash) -> bool {
        self.object_path(hash).is_file()
    }

    pub fn size_of(&self, hash: &Hash) -> Result<u64> {
        let path = self.object_path(hash);
        match fs::metadata(&path) {
            Ok(meta) => Ok(meta.len()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Error::Missing { hash: *hash }),
            Err(e) => Err(io_error(path, e)),
        }
    }

    pub fn alias_path(&self, namespace: &str, key: &str) -> PathBuf {
        let (shard, rest) = if key.len() > SHARD_LEN {
            key.split_at(SHARD_LEN)
        } else {
            ("_", key)
        };
        self.root
            .join(ALIASES_DIR)
            .join(namespace)
            .join(shard)
            .join(rest)
    }

    pub fn alias(&self, namespace: &str, key: &str) -> Result<Option<Hash>> {
        let path = self.alias_path(namespace, key);
        match fs::read_to_string(&path) {
            Ok(contents) => Ok(contents.trim().parse().ok()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(io_error(path, e)),
        }
    }

    pub fn set_alias(&self, namespace: &str, key: &str, hash: &Hash) -> Result<()> {
        let path = self.alias_path(namespace, key);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        fs::write(&path, hash.to_hex()).map_err(|e| io_error(path, e))
    }

    pub fn resolve_alias(&self, namespace: &str, key: &str) -> Result<Option<Hash>> {
        match self.alias(namespace, key)? {
            Some(hash) if self.contains(&hash) => Ok(Some(hash)),
            _ => Ok(None),
        }
    }

    pub fn writer(&self) -> Result<Writer> {
        Writer::new(self.clone())
    }

    pub fn insert_bytes(&self, bytes: &[u8]) -> Result<Hash> {
        let mut writer = self.writer()?;
        writer
            .write_all(bytes)
            .map_err(|e| io_error(writer.temp_path(), e))?;
        writer.commit()
    }

    pub fn insert_path(&self, source: &Path) -> Result<Hash> {
        let mut file = fs::File::open(source).map_err(|e| io_error(source, e))?;
        let mut writer = self.writer()?;
        io::copy(&mut file, &mut writer).map_err(|e| io_error(source, e))?;
        writer.commit()
    }

    pub fn read(&self, hash: &Hash) -> Result<Vec<u8>> {
        let path = self.object_path(hash);
        match fs::read(&path) {
            Ok(bytes) => Ok(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Err(Error::Missing { hash: *hash }),
            Err(e) => Err(io_error(path, e)),
        }
    }

    pub fn verify(&self, hash: &Hash) -> Result<()> {
        let path = self.object_path(hash);
        let mut file = match fs::File::open(&path) {
            Ok(file) => file,
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                return Err(Error::Missing { hash: *hash })
            }
            Err(e) => return Err(io_error(path, e)),
        };
        let mut hasher = blake3::Hasher::new();
        io::copy(&mut file, &mut hasher).map_err(|e| io_error(&path, e))?;
        let actual = Hash::from(hasher.finalize());
        if actual == *hash {
            Ok(())
        } else {
            Err(Error::Corrupt {
                expected: *hash,
                actual,
            })
        }
    }

    pub fn materialize(&self, hash: &Hash, dest: &Path, mode: Materialize) -> Result<Link> {
        if !self.contains(hash) {
            return Err(Error::Missing { hash: *hash });
        }
        let source = self.object_path(hash);

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        remove_if_present(dest)?;

        let link = self.place(&source, dest, mode)?;
        set_permissions(dest, mode)?;
        Ok(link)
    }

    fn place(&self, source: &Path, dest: &Path, mode: Materialize) -> Result<Link> {
        if reflink_copy::reflink(source, dest).is_ok() {
            return Ok(Link::Reflink);
        }

        if mode == Materialize::Shared {
            remove_if_present(dest)?;
            if fs::hard_link(source, dest).is_ok() {
                return Ok(Link::Hardlink);
            }
        }

        remove_if_present(dest)?;
        fs::copy(source, dest).map_err(|e| io_error(dest, e))?;
        Ok(Link::Copy)
    }

    pub fn gc(&self, live: &HashSet<Hash>) -> Result<GcStats> {
        let objects = self.root.join(OBJECTS_DIR);
        let mut stats = GcStats::default();

        for shard in read_dir(&objects)? {
            let shard = shard?;
            if !shard
                .file_type()
                .map_err(|e| io_error(shard.path(), e))?
                .is_dir()
            {
                continue;
            }
            for object in read_dir(&shard.path())? {
                let object = object?;
                let path = object.path();
                let Some(hash) = hash_from_object_path(&path) else {
                    continue;
                };
                let size = object.metadata().map_err(|e| io_error(&path, e))?.len();
                if live.contains(&hash) {
                    stats.retained_objects += 1;
                    stats.retained_bytes += size;
                } else {
                    fs::remove_file(&path).map_err(|e| io_error(&path, e))?;
                    stats.removed_objects += 1;
                    stats.removed_bytes += size;
                }
            }
            let _ = fs::remove_dir(shard.path());
        }

        Ok(stats)
    }
}

fn read_dir(path: &Path) -> Result<impl Iterator<Item = Result<fs::DirEntry>> + use<>> {
    let owned = path.to_path_buf();
    let entries = fs::read_dir(path).map_err(|e| io_error(path, e))?;
    Ok(entries.map(move |entry| entry.map_err(|e| io_error(&owned, e))))
}

fn hash_from_object_path(path: &Path) -> Option<Hash> {
    let name = path.file_name()?.to_str()?;
    let shard = path.parent()?.file_name()?.to_str()?;
    if shard.len() + name.len() != HASH_HEX_LEN {
        return None;
    }
    format!("{shard}{name}").parse().ok()
}

fn remove_if_present(path: &Path) -> Result<()> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(io_error(path, e)),
    }
}

#[cfg(unix)]
fn set_permissions(path: &Path, mode: Materialize) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let bits = match mode {
        Materialize::Shared => 0o444,
        Materialize::Private => 0o644,
    };
    fs::set_permissions(path, fs::Permissions::from_mode(bits)).map_err(|e| io_error(path, e))
}

#[cfg(not(unix))]
fn set_permissions(path: &Path, mode: Materialize) -> Result<()> {
    let meta = fs::metadata(path).map_err(|e| io_error(path, e))?;
    let mut perms = meta.permissions();
    perms.set_readonly(mode == Materialize::Shared);
    fs::set_permissions(path, perms).map_err(|e| io_error(path, e))
}

pub struct Writer {
    store: Store,
    temp_path: PathBuf,
    file: Option<fs::File>,
    hasher: blake3::Hasher,
    written: u64,
}

impl Writer {
    fn new(store: Store) -> Result<Self> {
        let unique = TMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temp_path = store
            .root
            .join(TMP_DIR)
            .join(format!("{}-{unique}", std::process::id()));
        let file = fs::File::create(&temp_path).map_err(|e| io_error(&temp_path, e))?;
        Ok(Self {
            store,
            temp_path,
            file: Some(file),
            hasher: blake3::Hasher::new(),
            written: 0,
        })
    }

    pub fn temp_path(&self) -> &Path {
        &self.temp_path
    }

    pub fn written(&self) -> u64 {
        self.written
    }

    pub fn hash(&self) -> Hash {
        Hash::from(self.hasher.finalize())
    }

    pub fn commit(mut self) -> Result<Hash> {
        let mut file = self.file.take().expect("writer used after commit");
        file.flush().map_err(|e| io_error(&self.temp_path, e))?;
        file.sync_all().map_err(|e| io_error(&self.temp_path, e))?;
        drop(file);

        let hash = self.hash();
        let dest = self.store.object_path(&hash);

        if dest.is_file() {
            remove_if_present(&self.temp_path)?;
            return Ok(hash);
        }

        if let Some(parent) = dest.parent() {
            fs::create_dir_all(parent).map_err(|e| io_error(parent, e))?;
        }
        fs::rename(&self.temp_path, &dest).map_err(|e| io_error(&dest, e))?;
        set_permissions(&dest, Materialize::Shared)?;
        Ok(hash)
    }
}

impl Write for Writer {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let file = self
            .file
            .as_mut()
            .ok_or_else(|| io::Error::other("writer used after commit"))?;
        let n = file.write(buf)?;
        self.hasher.update(&buf[..n]);
        self.written += n as u64;
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file.as_mut() {
            Some(file) => file.flush(),
            None => Ok(()),
        }
    }
}

impl Drop for Writer {
    fn drop(&mut self) {
        if self.file.is_some() {
            let _ = fs::remove_file(&self.temp_path);
        }
    }
}
