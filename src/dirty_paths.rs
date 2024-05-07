use error_stack::ResultExt;

use crate::{Result, TmsError};

pub trait DirtyUtf8Path {
    fn to_string(&self) -> Result<String>;
}

impl DirtyUtf8Path for std::path::PathBuf {
    fn to_string(&self) -> Result<String> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
impl DirtyUtf8Path for std::path::Path {
    fn to_string(&self) -> Result<String> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
impl DirtyUtf8Path for std::ffi::OsStr {
    fn to_string(&self) -> Result<String> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
