use std::io;
use std::path::PathBuf;

/// Why a health-core operation failed.
#[derive(Debug)]
pub enum Error {
    Invalid(String),
    Io(io::Error),
    Sqlite(rusqlite::Error),
    Log(localcore_log::Error),
    Blob(localcore_blob::Error),
    Json(serde_json::Error),
    MissingDatabase { path: PathBuf },
}

pub type Result<T> = std::result::Result<T, Error>;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Invalid(msg) => write!(f, "{msg}"),
            Error::Io(err) => write!(f, "{err}"),
            Error::Sqlite(err) => write!(f, "{err}"),
            Error::Log(err) => write!(f, "{err}"),
            Error::Blob(err) => write!(f, "{err}"),
            Error::Json(err) => write!(f, "{err}"),
            Error::MissingDatabase { path } => {
                write!(
                    f,
                    "query: database missing at {}; run rebuild",
                    path.display()
                )
            }
        }
    }
}

impl std::error::Error for Error {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Error::Io(err) => Some(err),
            Error::Sqlite(err) => Some(err),
            Error::Log(err) => Some(err),
            Error::Blob(err) => Some(err),
            Error::Json(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(err: io::Error) -> Self {
        Error::Io(err)
    }
}

impl From<rusqlite::Error> for Error {
    fn from(err: rusqlite::Error) -> Self {
        Error::Sqlite(err)
    }
}

impl From<localcore_log::Error> for Error {
    fn from(err: localcore_log::Error) -> Self {
        Error::Log(err)
    }
}

impl From<localcore_blob::Error> for Error {
    fn from(err: localcore_blob::Error) -> Self {
        Error::Blob(err)
    }
}

impl From<serde_json::Error> for Error {
    fn from(err: serde_json::Error) -> Self {
        Error::Json(err)
    }
}
