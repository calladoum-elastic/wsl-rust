use derive_more::{Display, From};
use wsl_sys::windows_core;

/// `WSL` crate specifc result type
pub type Result<T> = std::result::Result<T, Error>;

/// `ttd` crate specifc error type
#[derive(From, Debug, Display, Default)]
pub enum Error {
    #[default]
    Unknown,

    Generic(String),

    MissingRequiredValue(&'static str),
    NotFound,
    InitializationError,
    ConversionError,
    ForeignFunctionError {
        fname: &'static str,
    },

    #[from]
    IoError(std::io::Error),

    #[from]
    WindowsCoreError(windows_core::Error),

    #[from]
    RecvTimeoutError(std::sync::mpsc::RecvTimeoutError),
}

impl std::error::Error for Error {}
