//! End-to-End Example - but in Rust
//! * https://wsl.dev/api-reference/cpp/end-to-end-example/

use std::io::Write;
use std::process::ExitCode;
use std::time::Duration;

use wsl::ServiceComponent;
use wsl::container::{ContainerState, Signal};
use wsl::process::{OutputMode, ProcessBuilder};
use wsl::session::SessionBuilder;

fn main() -> ExitCode {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    match run() {
        Ok(code) => code,
        Err(e) => {
            log::error!("{e}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> wsl::Result<ExitCode> {
    // 0. Check prerequisites
    let missing = wsl::missing_components()?;
    if missing.contains(&ServiceComponent::SdkNeedsUpdate) {
        log::error!("This application is incompatible with the installed WSL runtime.");
        return Ok(ExitCode::FAILURE);
    }
    if !missing.is_empty() {
        log::error!("WSL components are missing ({missing:?}). Run: wsl --install/--update");
        return Ok(ExitCode::FAILURE);
    }

    let ver = wsl::version()?;
    log::info!("WSL version: {}.{}.{}", ver.major, ver.minor, ver.revision);

    // 1. Initialize and create a session
    let session = SessionBuilder::new("MyApp", r"C:\WslcData", false)
        .cpu_count(1)
        .memory_size_mb(1024)
        .build()?;

    session.start()?;

    // 2. Pull an image
    let image_url = "docker.io/library/alpine:latest";
    let image_name = image_url.rsplit("/").nth(0).ok_or(wsl::Error::default())?;

    session.pull_image(image_url)?;

    // 3. Configure an init process
    let init_process = ProcessBuilder::new(["/bin/echo", "Hello from WSL Container!"]).output_mode(OutputMode::Event);

    // 4. Configure and create a container
    let container = session
        .create_container(image_name)?
        .with_name("hello-container")
        .with_init_process(init_process)
        .build()?;

    // 5. Subscribe to init process events before starting
    let events = container.init_process().events()?;

    // 6. Start the container
    container.start()?;

    // 7. Wait for the init process to exit (30-second timeout)
    let exit_code = events
        .wait_for_exit(Duration::from_secs(30), |data| {
            let _ = std::io::stdout().write_all(data);
        })
        .unwrap_or(-1);
    log::info!("Process exited with code: {exit_code}");

    // 8. Clean up
    if container.state()? == ContainerState::Running {
        container.stop(Signal::Terminate, Duration::from_secs(10))?;
    }

    // container deletion and session termination are now handled through their scope
    Ok(ExitCode::SUCCESS)
}
