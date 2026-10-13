//! Reading and writing files inside a running container, without any volume mapping.
use std::io::{Read, Write};
use std::process::ExitCode;
use std::time::Duration;

use log::{error, info};
use wsl::container::DeleteOption;
use wsl::process::{OutputMode, ProcessBuilder};
use wsl::session::SessionBuilder;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    match run() {
        Ok(code) => code,
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> wsl::Result<ExitCode> {
    let image_url = "docker.io/library/alpine:latest";
    let image_name = image_url.rsplit("/").next().ok_or(wsl::Error::default())?;

    // 0. pre-flight checks
    if !wsl::missing_components()?.is_empty() {
        return Ok(ExitCode::FAILURE);
    }

    // 1. build the session, start the container
    let session = SessionBuilder::default().build()?;
    session.start()?;
    session.pull_image(image_url)?;

    let container = session
        .create_container(image_name)?
        .with_name("container-files")
        .with_init_process(ProcessBuilder::new(["sleep", "infinity"]))
        .with_delete_option(DeleteOption::Force)
        .build()?;
    container.start()?;

    // 2. try to read a file from FS
    let os_release = container.read_file("/etc/os-release")?;
    info!("/etc/os-release:\n{}", String::from_utf8_lossy(&os_release));

    // 3. write a file, then read it back
    let path = "/tmp/hello from windows.txt";
    let content = b"Hello from the Windows side!\n";
    container.write_file(path, content)?;
    let read_back = container.read_file(path)?;
    info!("{path:?} round-trip ok: {}", read_back == content);

    // 4. pipe data through a process's stdin/stdout
    let process: wsl::process::Process = container.create_process(
        ProcessBuilder::new(["tr", "a-z", "A-Z"])
            .output_mode(OutputMode::Stream)
            .expose_stdin(true),
    )?;
    let events = process.events()?;
    process.start()?;
    process.stdin()?.write_all(b"onweek rockz !!\n")?;

    let mut upper = String::new();
    process.stdout()?.read_to_string(&mut upper)?;
    let code = events.wait_for_exit(Duration::from_secs(10), |_| {});
    info!("tr exited with {code:?}: {}", upper.trim());

    Ok(ExitCode::SUCCESS)
}
