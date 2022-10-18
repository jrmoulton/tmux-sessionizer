use error_stack::{IntoReport, Result, ResultExt};

use crate::TmsError;

pub(crate) trait DirtyUtf8Path {
    fn to_string(&self) -> Result<String, TmsError>;
}
impl DirtyUtf8Path for std::path::PathBuf {
    fn to_string(&self) -> Result<String, TmsError> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .into_report()
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
impl DirtyUtf8Path for std::path::Path {
    fn to_string(&self) -> Result<String, TmsError> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .into_report()
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
impl DirtyUtf8Path for std::ffi::OsStr {
    fn to_string(&self) -> Result<String, TmsError> {
        Ok(self
            .to_str()
            .ok_or(TmsError::NonUtf8Path)
            .into_report()
            .attach_printable("Not a valid utf8 path")?
            .to_string())
    }
}
