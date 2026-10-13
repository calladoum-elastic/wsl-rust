use std::path::PathBuf;
use std::{fmt::Debug, os::windows::ffi::OsStrExt};

use log::{debug, error};

use wsl_sys::windows_core::HSTRING;

use crate::prelude::*;
use crate::utils::{gen_random_string, hstring};
use crate::{container::ContainerBuilder, raw};

const SESSION_DEFAULT_MEMORY_SIZE_MB: u32 = 1024;
const SESSION_DEFAULT_CPU_COUNT: u32 = 1;
const SESSION_DEFAULT_SESSION_NAME_LENGTH: usize = 10;
const SESSION_DEFAULT_SESSION_NAME_PREFIX: &str = "Wsl-";

/// Configuration for a new [`Session`].
#[derive(Clone, Debug)]
pub struct SessionBuilder {
    name: String,
    storage_path: PathBuf,
    cpu_count: u32,
    memory_size_mb: u32,
    enable_gpu: bool,
    delete_vhd_on_exit: bool,
}

impl SessionBuilder {
    /// `storage_path` is where the session keeps its images and container data.
    pub fn new(name: impl Into<String>, storage_path: impl Into<PathBuf>, delete_vhd_on_exit: bool) -> Self {
        Self {
            name: name.into(),
            storage_path: storage_path.into(),
            cpu_count: SESSION_DEFAULT_CPU_COUNT,
            memory_size_mb: SESSION_DEFAULT_MEMORY_SIZE_MB,
            enable_gpu: false,
            delete_vhd_on_exit,
        }
    }

    pub fn cpu_count(mut self, count: u32) -> Self {
        self.cpu_count = count;
        self
    }

    pub fn enable_gpu(mut self, value: bool) -> Self {
        self.enable_gpu = value;
        self
    }

    pub fn memory_size_mb(mut self, mb: u32) -> Self {
        self.memory_size_mb = mb;
        self
    }

    pub fn build(self) -> Result<Session> {
        wsl_sys::init()?; // make sure we're COM-initialized
        let path: Vec<u16> = self.storage_path.as_os_str().encode_wide().collect();
        let s = raw::SessionSettings::CreateInstance(&hstring(&self.name), &HSTRING::from_wide(&path))?;
        s.SetCpuCount(Some(self.cpu_count))?;
        s.SetMemorySizeInMB(Some(self.memory_size_mb))?;
        s.SetEnableGpu(self.enable_gpu)?;

        Ok(Session {
            raw: raw::Session::CreateInstance(&s)?,
            name: self.name,
            cpu_count: self.cpu_count as usize,
            memory_size: self.memory_size_mb as usize,
            storage_path: self.storage_path,
            delete_vhd_on_exit: self.delete_vhd_on_exit,
        })
    }
}

impl Default for SessionBuilder {
    fn default() -> Self {
        let rand_id = gen_random_string(SESSION_DEFAULT_SESSION_NAME_LENGTH);
        SessionBuilder::new(
            SESSION_DEFAULT_SESSION_NAME_PREFIX.to_string() + &rand_id,
            std::env::temp_dir().join(rand_id),
            true,
        )
    }
}

/// A WSL [`Session`] holds all info regarding the images and containers. A [`Session`] cannot be built
/// directly, use [`SessionBuilder`] to prepare a working session.
pub struct Session {
    raw: raw::Session,
    name: String,
    cpu_count: usize,
    memory_size: usize,
    storage_path: PathBuf,
    delete_vhd_on_exit: bool,
}

impl Session {
    pub fn start(&self) -> Result<()> {
        Ok(self.raw.Start()?)
    }

    pub fn pull_image(&self, reference: &str) -> Result<()> {
        wsl_sys::init()?;
        debug!("Pulling image {reference:?}");
        let opts = raw::PullImageOptions::CreateInstance(&hstring(reference))?;
        Ok(self.raw.PullImage(&opts)?)
    }

    pub fn create_container(&self, image: impl Into<String>) -> Result<ContainerBuilder<'_>> {
        Ok(ContainerBuilder::new(image, &self.raw))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        debug!("Stopping {self:?}...");
        if let Err(e) = self.raw.Terminate() {
            error!("Error while terminating session {self:?}: {e:?}");
            return;
        }

        if self.delete_vhd_on_exit
            && let Err(e) = std::fs::remove_dir_all(self.storage_path.as_path())
        {
            error!("Error while deleting VHD file: {e:?}");
        }

        debug!("Stopped {self:?} successfully");
    }
}

impl Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("name", &self.name)
            .field("cpu_count", &self.cpu_count)
            .field("memory_size", &self.memory_size)
            .field("storage_path", &self.storage_path)
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_builder_defaults() {
        let s = SessionBuilder::new("TestApp", r"C:\TestFolder", false);
        assert_eq!(s.name, "TestApp");
        assert_eq!(s.storage_path, PathBuf::from(r"C:\TestFolder"));
        assert_eq!(s.cpu_count, SESSION_DEFAULT_CPU_COUNT);
        assert_eq!(s.memory_size_mb, SESSION_DEFAULT_MEMORY_SIZE_MB);
    }

    #[test]
    fn settings_builder_sets_fields() {
        let s = SessionBuilder::new("TestApp", r"C:\TestFolder", false)
            .cpu_count(4)
            .memory_size_mb(1024);
        assert_eq!(s.cpu_count, 4);
        assert_eq!(s.memory_size_mb, 1024);
    }
}
