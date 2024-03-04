use error_stack::{Result, ResultExt};

use crate::error::TmsError;

pub trait DirtyUtf8Path {
    fn to_string(&self) -> Result<String, TmsError>;
}

impl DirtyUtf8Path for std::path::PathBuf {
    fn to_string(&self) -> Result<String, TmsError> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
impl DirtyUtf8Path for std::path::Path {
    fn to_string(&self) -> Result<String, TmsError> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
impl DirtyUtf8Path for std::ffi::OsStr {
    fn to_string(&self) -> Result<String, TmsError> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
