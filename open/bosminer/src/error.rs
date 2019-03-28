//! The rurminer errors

use failure::{Backtrace, Context, Fail};
use std::fmt::{self, Display};
use std::io;

#[derive(Debug)]
pub struct Error {
    inner: Context<ErrorKind>,
}

#[derive(Clone, Eq, PartialEq, Debug, Fail)]
pub enum ErrorKind {
    /// Standard input/output error.
    #[fail(display = "IO error: {}", _0)]
    Io(String),

    /// General error used for more specific input/output error.
    #[fail(display = "General error: {}", _0)]
    General(String),

    /// Unexpected version of something.
    #[fail(display = "Unexpected {} version: {}, expected: {}", _0, _1, _2)]
    UnexpectedVersion(String, String, String),

    /// Error concerning hashboard with specific index.
    #[fail(display = "Hashboard {}: {}", _0, _1)]
    Hashboard(usize, String),

    /// Error concerning hashchip.
    #[fail(display = "Hashchip error: {}", _0)]
    Hashchip(String),

    /// Work or command FIFO timeout.
    #[fail(display = "FIFO error: {}: {}", _0, _1)]
    Fifo(Fifo, String),

    /// Baud rate errors.
    #[fail(display = "Baud rate error: {}", _0)]
    BaudRate(String),

    /// I2C errors.
    #[fail(display = "I2C error: {}", _0)]
    I2c(String),
}

#[derive(Clone, Eq, PartialEq, Debug, Fail)]
pub enum Fifo {
    #[fail(display = "timed out")]
    TimedOut(usize),
}

/// Implement Fail trait instead of use Derive to get more control over custom type.
/// The main advantage is customization of Context type which allows conversion of
/// any error types to this custom error with general error kind by calling context
/// method on any result type.
impl Fail for Error {
    fn cause(&self) -> Option<&Fail> {
        self.inner.cause()
    }

    fn backtrace(&self) -> Option<&Backtrace> {
        self.inner.backtrace()
    }
}

impl Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        Display::fmt(&self.inner, f)
    }
}

impl Error {
    pub fn kind(&self) -> ErrorKind {
        self.inner.get_context().clone()
    }
}

impl From<ErrorKind> for Error {
    fn from(kind: ErrorKind) -> Self {
        Self {
            inner: Context::new(kind),
        }
    }
}

impl From<Context<ErrorKind>> for Error {
    fn from(inner: Context<ErrorKind>) -> Self {
        Self { inner }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        let msg = e.to_string();
        Self {
            inner: e.context(ErrorKind::Io(msg)),
        }
    }
}

impl From<Context<&str>> for Error {
    fn from(context: Context<&str>) -> Self {
        Self {
            inner: context.map(|info| ErrorKind::General(info.to_string())),
        }
    }
}

impl From<Context<String>> for Error {
    fn from(context: Context<String>) -> Self {
        Self {
            inner: context.map(|info| ErrorKind::General(info)),
        }
    }
}

/// A specialized `Result` type bound to [`Error`].
pub type Result<T> = std::result::Result<T, Error>;