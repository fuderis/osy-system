use crate::prelude::DynError;
use macron::{Display, Error, From};

// The error
#[derive(Debug, Display, Error, From)]
pub enum Error {
    #[display(fmt = "{0}")]
    Custom(String),

    #[display(fmt = "{0}: {1}")]
    Titled(String, DynError),

    #[display(fmt = "Connection to the client is closed.")]
    ConnectionClosed,

    #[from(skip)]
    #[display(fmt = "A non-existing tool was called `{0}`")]
    UnknownTool(String),

    #[display(fmt = "This feature is not supported on your OS.")]
    UnsupportedOS,
}
