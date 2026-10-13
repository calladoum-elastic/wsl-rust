//! Basic example of connecting to an interactive process
use std::io::{BufRead, Write};
use std::process::ExitCode;
use std::time::Duration;

use log::{debug, error, info, warn};
use wsl::container::ContainerState;
use wsl::container::Signal;
use wsl::process::{OutputMode, ProcessBuilder};
use wsl::session::SessionBuilder;

const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("debug")).init();

    match run() {
        Ok(code) => code,
        Err(e) => {
            error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> wsl::Result<ExitCode> {
    let image_url = "docker.io/library/ubuntu:latest";
    let image_name = image_url.rsplit("/").nth(0).ok_or(wsl::Error::default())?;
    let container_name = "interactive-bash";

    let missing = wsl::missing_components()?;
    if !missing.is_empty() {
        return Ok(ExitCode::FAILURE);
    }

    let session = SessionBuilder::default().cpu_count(2).memory_size_mb(1024).build()?;
    session.start()?;
    info!("session started: {:?}", &session);

    session.pull_image(image_url)?;

    let container = session
        .create_container(image_name)?
        .with_name(container_name)
        .with_init_process(ProcessBuilder::new(["sleep", "infinity"]))
        .with_delete_option(wsl::container::DeleteOption::Force)
        .with_volume(r"C:\", "/mnt/c", false)
        // .with_volume(r"C:\Users\chris", "/mnt/home", false)
        .with_port_mapping(8080, 80, wsl::container::PortProtocol::Tcp)
        .build()?;
    container.start()?;

    info!("Type a command, or `exit` / Ctrl-Z + Enter to quit.");

    let stdin = std::io::stdin();
    loop {
        print!("bash$ ");
        std::io::stdout().flush().ok();

        let mut line = String::new();
        if stdin.lock().read_line(&mut line).unwrap_or(0) == 0 {
            break; // EOF
        }
        let line = line.trim();
        match line {
            "" => continue,
            "exit" | "quit" => break,
            _ => {}
        }

        let script = format!("{{ {line}\n}} 2>&1");
        let process =
            container.create_process(ProcessBuilder::new(["bash", "-c", &script]).output_mode(OutputMode::Event))?;
        let events = process.events()?;
        process.start()?;

        let code = events.wait_for_exit(COMMAND_TIMEOUT, |data| {
            let mut out = std::io::stdout();
            let _ = out.write_all(data);
            let _ = out.flush();
        });
        match code {
            Some(0) => {}
            Some(code) => warn!("[exit status {code}]"),
            None => error!("[timed out after {}s]", COMMAND_TIMEOUT.as_secs()),
        }
    }

    if container.state()? == ContainerState::Running {
        container.stop(Signal::Kill, Duration::from_secs(10))?;
    }

    debug!("Terminating {session:?}");
    Ok(ExitCode::SUCCESS)
}
