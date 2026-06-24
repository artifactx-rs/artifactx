use std::fmt;

#[derive(Debug)]
pub enum XmlError {
    ParseError(String),
    InvalidData(String),
    IoError(std::io::Error),
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ParseError(msg) => write!(f, "XML parse error: {msg}"),
            Self::InvalidData(msg) => write!(f, "Invalid XML data: {msg}"),
            Self::IoError(e) => write!(f, "IO error: {e}"),
        }
    }
}

impl std::error::Error for XmlError {}

impl From<std::io::Error> for XmlError {
    fn from(err: std::io::Error) -> Self {
        Self::IoError(err)
    }
}
