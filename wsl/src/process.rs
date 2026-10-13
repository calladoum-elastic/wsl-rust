use std::io::{Read, Write};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use wsl_sys::bindings::Windows::Storage::Streams as streams;
use wsl_sys::windows_collections::IVector;
use wsl_sys::windows_core::{EventRevoker, HSTRING};

pub use std::sync::mpsc::RecvTimeoutError;

use crate::prelude::*;
use crate::raw;
use crate::utils::hstring;

/// What to do with a process's standard output.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum OutputMode {
    Invalid,
    #[default]
    Discard,
    /// Expose output as readable streams through [`Process::stdout`] and [`Process::stderr`].
    Stream,
    /// Deliver output through [`Process::events`].
    Event,
}

#[derive(Clone, Debug)]
pub struct ProcessBuilder {
    args: Vec<String>,
    output_mode: OutputMode,
    expose_stdin: bool,
}

impl ProcessBuilder {
    /// `args[0]` is the executable; the rest are its arguments.
    pub fn new<I, S>(args: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            args: args.into_iter().map(Into::into).collect(),
            output_mode: OutputMode::default(),
            expose_stdin: false,
        }
    }

    pub fn output_mode(mut self, mode: OutputMode) -> Self {
        self.output_mode = mode;
        self
    }

    pub fn expose_stdin(mut self, enable: bool) -> Self {
        self.expose_stdin = enable;
        self
    }

    pub fn build(self, container: &raw::Container) -> Result<Process> {
        Ok(Process::from_raw(
            container.CreateProcess(&self.settings()?)?,
            self.output_mode,
        ))
    }

    pub(crate) fn configured_output_mode(&self) -> OutputMode {
        self.output_mode
    }

    pub(crate) fn settings(&self) -> Result<raw::ProcessSettings> {
        wsl_sys::init()?;
        let s = raw::ProcessSettings::new()?;
        s.SetOutputMode(self.output_mode.into())?;
        s.SetEnableStandardInput(self.expose_stdin)?;
        let argv: Vec<HSTRING> = self.args.iter().map(hstring).collect();
        s.SetCommandLine(&IVector::from(argv))?;
        Ok(s)
    }
}

impl From<raw::ProcessOutputMode> for OutputMode {
    fn from(value: raw::ProcessOutputMode) -> Self {
        match value {
            raw::ProcessOutputMode::Discard => OutputMode::Discard,
            raw::ProcessOutputMode::Stream => OutputMode::Stream,
            raw::ProcessOutputMode::Event => OutputMode::Event,
            _ => OutputMode::Invalid,
        }
    }
}

impl From<OutputMode> for raw::ProcessOutputMode {
    fn from(value: OutputMode) -> Self {
        match value {
            OutputMode::Discard | OutputMode::Invalid => raw::ProcessOutputMode::Discard,
            OutputMode::Stream => raw::ProcessOutputMode::Stream,
            OutputMode::Event => raw::ProcessOutputMode::Event,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProcessState {
    Invalid,
    Running,
    Exited,
    Signalled,
}

/// A process running inside a container.
#[derive(Debug)]
pub struct Process {
    raw: raw::Process,
    output_mode: OutputMode,
}

impl Process {
    pub(crate) fn from_raw(p: raw::Process, output_mode: OutputMode) -> Self {
        Self { raw: p, output_mode }
    }

    /// Starts a process created with [`Container::create_process`](crate::container::Container::create_process).
    pub fn start(&self) -> Result<()> {
        Ok(self.raw.Start()?)
    }

    pub fn state(&self) -> Result<ProcessState> {
        Ok(match self.raw.State()? {
            raw::ProcessState::Running => ProcessState::Running,
            raw::ProcessState::Exited => ProcessState::Exited,
            raw::ProcessState::Signalled => ProcessState::Signalled,
            _ => ProcessState::Invalid,
        })
    }

    pub fn exit_code(&self) -> Result<i32> {
        Ok(self.raw.ExitCode()?)
    }

    pub fn events(&self) -> Result<ProcessEvents> {
        let (tx, rx) = mpsc::channel();
        let output = if self.output_mode == OutputMode::Event {
            let tx_out = tx.clone();
            Some(self.raw.OutputReceived(move |data| {
                let _ = tx_out.send(ProcessEvent::Output(data.to_vec()));
            })?)
        } else {
            None
        };
        let exited = self.raw.Exited(move |code| {
            let _ = tx.send(ProcessEvent::Exited(code));
        })?;
        Ok(ProcessEvents {
            rx,
            _output: output,
            _exited: exited,
        })
    }

    pub fn stdin(&self) -> Result<ProcessStdin> {
        let writer = streams::DataWriter::CreateDataWriter(&self.raw.GetInputStream()?)?;
        Ok(ProcessStdin { writer: Some(writer) })
    }

    pub fn stdout(&self) -> Result<ProcessOutput> {
        ProcessOutput::new(self.raw.GetOutputStream(raw::ProcessOutputHandle::StandardOutput)?)
    }

    pub fn stderr(&self) -> Result<ProcessOutput> {
        ProcessOutput::new(self.raw.GetOutputStream(raw::ProcessOutputHandle::StandardError)?)
    }
}

fn to_io_error(e: wsl_sys::windows_core::Error) -> std::io::Error {
    std::io::Error::other(e)
}

/// The standard input of a [`Process`], obtained from [`Process::stdin`].
pub struct ProcessStdin {
    writer: Option<streams::DataWriter>,
}

impl ProcessStdin {
    /// Flushes pending data and closes the stream, signalling end-of-file to the process.
    pub fn close(mut self) -> Result<()> {
        self.close_inner()
    }

    fn close_inner(&mut self) -> Result<()> {
        if let Some(writer) = self.writer.take() {
            writer.FlushAsync()?.join()?;
            // Closing the writer closes the underlying stream too.
            writer.Close()?;
        }
        Ok(())
    }

    fn writer(&self) -> std::io::Result<&streams::DataWriter> {
        self.writer
            .as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::BrokenPipe, "stdin is closed"))
    }
}

impl Write for ProcessStdin {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        let writer = self.writer()?;
        writer.WriteBytes(buf).map_err(to_io_error)?;
        writer.StoreAsync().and_then(|op| op.join()).map_err(to_io_error)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        self.writer()?
            .FlushAsync()
            .and_then(|op| op.join())
            .map(|_| ())
            .map_err(to_io_error)
    }
}

impl Drop for ProcessStdin {
    fn drop(&mut self) {
        if let Err(e) = self.close_inner() {
            log::error!("Error while closing process stdin: {e:?}");
        }
    }
}

impl std::fmt::Debug for ProcessStdin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessStdin")
            .field("closed", &self.writer.is_none())
            .finish()
    }
}

pub struct ProcessOutput {
    stream: streams::IInputStream,
}

const MAX_READ_CHUNK: u32 = 64 * 1024;

impl ProcessOutput {
    fn new(stream: streams::IInputStream) -> Result<Self> {
        Ok(Self { stream })
    }
}

impl Read for ProcessOutput {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let count = u32::try_from(buf.len()).unwrap_or(u32::MAX).min(MAX_READ_CHUNK);
        let chunk = (|| {
            let buffer = streams::Buffer::Create(count)?;
            self.stream
                .ReadAsync(&buffer, count, streams::InputStreamOptions::None)?
                .join()
        })()
        .map_err(to_io_error)?;

        let n = chunk.Length().map_err(to_io_error)? as usize;
        if n > 0 {
            streams::DataReader::FromBuffer(&chunk)
                .and_then(|reader| reader.ReadBytes(&mut buf[..n]))
                .map_err(to_io_error)?;
        }
        Ok(n)
    }
}

impl std::fmt::Debug for ProcessOutput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProcessOutput").finish_non_exhaustive()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProcessEvent {
    Output(Vec<u8>),
    Exited(i32),
}

/// A subscription to a [`Process`]'s events.
pub struct ProcessEvents {
    rx: mpsc::Receiver<ProcessEvent>,
    _output: Option<EventRevoker>,
    _exited: EventRevoker,
}

impl ProcessEvents {
    pub fn recv_timeout(&self, timeout: Duration) -> Result<ProcessEvent> {
        Ok(self.rx.recv_timeout(timeout)?)
    }

    pub fn wait_for_exit(&self, timeout: Duration, mut on_output: impl FnMut(&[u8])) -> Option<i32> {
        let deadline = Instant::now() + timeout;
        loop {
            match self.rx.recv_timeout(deadline.saturating_duration_since(Instant::now())) {
                Ok(ProcessEvent::Output(data)) => on_output(&data),
                Ok(ProcessEvent::Exited(code)) => return Some(code),
                Err(_) => return None,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_collect_args() {
        let s = ProcessBuilder::new(["/bin/echo", "hi"]);
        assert_eq!(s.args, ["/bin/echo", "hi"]);
        assert_eq!(s.output_mode, OutputMode::Discard);
        assert!(!s.expose_stdin);
    }

    #[test]
    fn settings_accept_owned_strings() {
        let s = ProcessBuilder::new(vec![String::from("ls"), String::from("-l")])
            .output_mode(OutputMode::Event)
            .expose_stdin(true);
        assert_eq!(s.args, ["ls", "-l"]);
        assert_eq!(s.output_mode, OutputMode::Event);
        assert!(s.expose_stdin);
    }

    #[test]
    fn output_mode_defaults_to_discard() {
        assert_eq!(OutputMode::default(), OutputMode::Discard);
    }
}
