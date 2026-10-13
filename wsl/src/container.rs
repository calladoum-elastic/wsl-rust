use core::convert::Into;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::time::Duration;

use log::{debug, error};
use wsl_sys::windows_collections::IVector;
use wsl_sys::windows_time::TimeSpan;

use crate::prelude::*;
use crate::process::{OutputMode, Process, ProcessBuilder, ProcessEvents};
use crate::raw;
use crate::utils::hstring;

/// How long [`Container::read_file`] / [`Container::write_file`] wait for their helper process to exit.
const FILE_OPERATION_TIMEOUT: Duration = Duration::from_secs(30);

/// Configuration for a new container.
#[derive(Clone, Debug)]
pub struct ContainerBuilder<'a> {
    image: String,
    name: Option<String>,
    init_process: Option<ProcessBuilder>,
    volumes: Vec<Volume>,
    delete_option: DeleteOption,
    raw: &'a raw::Session,
    port_mappings: Vec<PortMapping>,
}

/// A Windows directory mounted into a container.
#[derive(Clone, Debug)]
struct Volume {
    windows_path: String,
    container_path: String,
    read_only: bool,
}

impl TryFrom<&Volume> for raw::ContainerVolume {
    type Error = crate::Error;

    fn try_from(value: &Volume) -> Result<Self> {
        Ok(raw::ContainerVolume::CreateInstance(
            &hstring(&value.windows_path),
            &hstring(&value.container_path),
            value.read_only,
        )?)
    }
}

/// A port mapping associated to the container
#[derive(Clone, Debug)]
struct PortMapping {
    windows_port: u16,
    container_port: u16,
    protocol: PortProtocol,
}

#[derive(Clone, Debug)]
pub enum PortProtocol {
    Invalid,
    Tcp,
    Udp,
}

impl From<raw::PortProtocol> for PortProtocol {
    fn from(value: raw::PortProtocol) -> Self {
        match value {
            raw::PortProtocol::TCP => PortProtocol::Tcp,
            raw::PortProtocol::UDP => PortProtocol::Udp,
            _ => PortProtocol::Invalid,
        }
    }
}

impl TryFrom<PortProtocol> for raw::PortProtocol {
    type Error = crate::Error;

    fn try_from(value: PortProtocol) -> Result<Self> {
        match value {
            PortProtocol::Tcp => Ok(raw::PortProtocol::TCP),
            PortProtocol::Udp => Ok(raw::PortProtocol::UDP),
            _ => Err(Error::ConversionError),
        }
    }
}

impl TryFrom<raw::ContainerPortMapping> for PortMapping {
    type Error = crate::Error;

    fn try_from(v: raw::ContainerPortMapping) -> Result<Self> {
        Ok(PortMapping {
            windows_port: v.WindowsPort()?,
            container_port: v.ContainerPort()?,
            protocol: v.Protocol()?.into(),
        })
    }
}

impl TryFrom<&PortMapping> for raw::ContainerPortMapping {
    type Error = crate::Error;

    fn try_from(v: &PortMapping) -> Result<Self> {
        Ok(raw::ContainerPortMapping::CreateInstance(
            v.windows_port,
            v.container_port,
            v.protocol.clone().try_into()?,
        )?)
    }
}

impl<'a> ContainerBuilder<'a> {
    pub fn new(image: impl Into<String>, raw: &'a raw::Session) -> Self {
        Self {
            image: image.into(),
            name: None,
            init_process: None,
            delete_option: DeleteOption::None,
            raw,
            volumes: Vec::new(),
            port_mappings: Vec::new(),
        }
    }

    pub fn with_volume(
        mut self,
        windows_path: impl Into<String>,
        container_path: impl Into<String>,
        read_only: bool,
    ) -> Self {
        self.volumes.push(Volume {
            windows_path: windows_path.into(),
            container_path: container_path.into(),
            read_only,
        });
        self
    }

    pub fn with_name(mut self, name: impl Into<String>) -> Self {
        self.name = Some(name.into());
        self
    }

    pub fn with_init_process(mut self, init: ProcessBuilder) -> Self {
        self.init_process = Some(init);
        self
    }

    pub fn with_delete_option(mut self, option: DeleteOption) -> Self {
        self.delete_option = option;
        self
    }

    pub fn with_port_mapping(mut self, src: u16, dst: u16, proto: PortProtocol) -> Self {
        self.port_mappings.push(PortMapping {
            windows_port: src,
            container_port: dst,
            protocol: proto,
        });
        self
    }

    pub fn build(self) -> Result<Container> {
        wsl_sys::init()?;
        let settings = raw::ContainerSettings::CreateInstance(&hstring(&self.image))?;
        if let Some(name) = &self.name {
            settings.SetName(&hstring(name))?;
        }

        let process_builder = self
            .init_process
            .as_ref()
            .ok_or(Error::MissingRequiredValue("init_process"))?;
        settings.SetInitProcess(&process_builder.settings()?)?;

        if !self.volumes.is_empty() {
            let volumes: Vec<Option<raw::ContainerVolume>> = self
                .volumes
                .iter()
                .map(|volume| -> Option<raw::ContainerVolume> { volume.try_into().ok() })
                .collect();
            settings.SetVolumes(&IVector::<raw::ContainerVolume>::from(volumes))?;
        }

        if !self.port_mappings.is_empty() {
            // Default network mode is None, adding a port mapping implicitly means we need to switch to Bridged
            debug!("switching networking mode: None -> Bridge");
            settings.SetNetworkingMode(Some(raw::ContainerNetworkingMode::Bridged))?;

            let port_mappings: Vec<Option<raw::ContainerPortMapping>> = self
                .port_mappings
                .iter()
                .map(|mapping| -> Option<raw::ContainerPortMapping> { mapping.try_into().ok() })
                .collect();
            settings.SetPortMappings(&IVector::<raw::ContainerPortMapping>::from(port_mappings))?;

            debug!("port mappings added: {:?}", self.port_mappings);
        }

        let container = self.raw.CreateContainer(&settings)?;
        let process = Process::from_raw(container.InitProcess()?, process_builder.configured_output_mode());

        let res = Container {
            raw: container,
            init_process: process,
            delete_option: self.delete_option,
            image: self.image,
            name: self.name.ok_or(Error::MissingRequiredValue("container_name"))?,
            volumes: self.volumes,
            port_mappings: self.port_mappings,
        };

        debug!("New container added: {res:?}");
        Ok(res)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContainerState {
    Invalid,
    Created,
    Running,
    Exited,
    Deleted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Signal {
    None,
    Hangup,
    Interrupt,
    Quit,
    Kill,
    Terminate,
}

impl From<Signal> for raw::Signal {
    fn from(s: Signal) -> Self {
        match s {
            Signal::None => raw::Signal::None,
            Signal::Hangup => raw::Signal::SIGHUP,
            Signal::Interrupt => raw::Signal::SIGINT,
            Signal::Quit => raw::Signal::SIGQUIT,
            Signal::Kill => raw::Signal::SIGKILL,
            Signal::Terminate => raw::Signal::SIGTERM,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DeleteOption {
    /// Leave as-is
    #[default]
    None,
    /// Delete even if the container is still running.
    Force,
}

impl From<DeleteOption> for raw::DeleteContainerOption {
    fn from(value: DeleteOption) -> Self {
        match value {
            DeleteOption::None => raw::DeleteContainerOption::None,
            DeleteOption::Force => raw::DeleteContainerOption::Force,
        }
    }
}

/// A container created in a [`Session`](crate::session::Session).
pub struct Container {
    raw: raw::Container,
    delete_option: DeleteOption,
    image: String,
    name: String,
    init_process: Process,
    volumes: Vec<Volume>,
    port_mappings: Vec<PortMapping>,
}

impl Container {
    pub fn id(&self) -> Result<String> {
        Ok(String::from_utf16_lossy(&self.raw.Id()?))
    }

    /// The container's initial process. A container **must** have one to be spawn, and the container
    /// needs to subscribe to its events before calling [`start`](Self::start).
    pub fn init_process(&self) -> &Process {
        &self.init_process
    }

    pub fn start(&self) -> Result<()> {
        Ok(self.raw.Start()?)
    }

    pub fn create_process(&self, builder: ProcessBuilder) -> Result<Process> {
        builder.build(&self.raw)
    }

    /// HACK (calladoum) I didn't find any API for doing this cleanly so we use `sh -c cat` instead
    ///
    pub fn read_file(&self, path: &str) -> Result<Vec<u8>> {
        let process = self.create_process(ProcessBuilder::new(["cat", "--", path]).output_mode(OutputMode::Stream))?;
        let events = process.events()?;
        process.start()?;

        let mut content = Vec::new();
        process.stdout()?.read_to_end(&mut content)?;
        Self::wait_for_success(&process, &events, &format!("cat {path}"))?;
        Ok(content)
    }

    /// HACK (calladoum) Same
    pub fn write_file(&self, path: impl Into<PathBuf>, content: &[u8]) -> Result<()> {
        let path = path.into();
        let process = self.create_process(
            ProcessBuilder::new([
                "sh",
                "-c",
                r#"cat > "$0""#,
                path.to_str().ok_or(Error::ConversionError)?,
            ])
            .output_mode(OutputMode::Stream)
            .expose_stdin(true),
        )?;
        let events = process.events()?;
        process.start()?;

        process.stdin()?.write_all(content)?;
        Self::wait_for_success(&process, &events, &format!("write {}", path.display()))
    }

    fn wait_for_success(process: &Process, events: &ProcessEvents, what: &str) -> Result<()> {
        let mut stderr = String::new();
        process.stderr()?.read_to_string(&mut stderr)?;
        match events.wait_for_exit(FILE_OPERATION_TIMEOUT, |_| {}) {
            Some(0) => Ok(()),
            Some(code) => Err(Error::Generic(format!(
                "`{what}` exited with code {code}: {}",
                stderr.trim()
            ))),
            None => Err(Error::Generic(format!("`{what}` timed out"))),
        }
    }

    pub fn state(&self) -> Result<ContainerState> {
        Ok(self.raw.State()?.into())
    }

    /// Signals to the container, waiting up to `timeout` for it to stop.
    pub fn stop(&self, signal: Signal, timeout: Duration) -> Result<()> {
        let millis = i64::try_from(timeout.as_millis()).unwrap_or(i64::MAX);
        Ok(self.raw.Stop(signal.into(), TimeSpan::from_millis(millis))?)
    }
}

impl Drop for Container {
    fn drop(&mut self) {
        debug!("Stopping {self:?}...");
        if let Err(e) = self.raw.Delete(self.delete_option.into()) {
            error!("Error while deleting container {self:?}: {e:?}");
            return;
        }
        debug!("Stopped {self:?} successfully");
    }
}

impl std::fmt::Debug for Container {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Container")
            .field("name", &self.name)
            .field("image", &self.image)
            .field("process", &self.init_process)
            .field("volumes", &self.volumes)
            .field("port_mappings", &self.port_mappings)
            .field("state", &self.state())
            .finish_non_exhaustive()
    }
}

impl From<raw::ContainerState> for ContainerState {
    fn from(value: raw::ContainerState) -> Self {
        match value {
            raw::ContainerState::Created => ContainerState::Created,
            raw::ContainerState::Running => ContainerState::Running,
            raw::ContainerState::Exited => ContainerState::Exited,
            raw::ContainerState::Deleted => ContainerState::Deleted,
            _ => ContainerState::Invalid,
        }
    }
}
