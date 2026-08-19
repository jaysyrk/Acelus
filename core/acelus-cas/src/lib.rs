mod hash;
mod store;

pub use hash::{Hash, ParseHashError, HASH_HEX_LEN, HASH_LEN};
pub use store::{GcStats, Link, Materialize, Store, Writer};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("content addressed store i/o failed at {path}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },

    #[error("object {hash} is not in the store")]
    Missing { hash: Hash },

    #[error("object {expected} was corrupt on disk; its contents hash to {actual}")]
    Corrupt { expected: Hash, actual: Hash },

    #[error("refusing to open a store at {path}: it exists but is not a directory")]
    NotADirectory { path: std::path::PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;

pub(crate) fn io_error(path: impl Into<std::path::PathBuf>, source: std::io::Error) -> Error {
    Error::Io {
        path: path.into(),
        source,
    }
}
