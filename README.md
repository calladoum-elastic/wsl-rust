# `wsl-rust`

Rust bindings for the [Windows Subsystem for Linux (WSL) SDK](https://wsl.dev/api-reference).

## Features

- `embed-runtime` (default: `true`): embeds the DLL and Riddle file in the generated binary

## Build

Requires
- Rust 1.98
- WSL2 (>= 3.0+) installed
    `wsl --install`

## Examples

### Basic API usage

A simple working example

```rust
use wsl::container::Signal;
use wsl::process::ProcessBuilder;
use wsl::session::SessionBuilder;

fn run() -> wsl::Result<ExitCode>  {
    let session = SessionBuilder::new("MyApp", r"C:\WslcData", false)
        .cpu_count(1)
        .memory_size_mb(1024)
        .build()?;
    session.start()?;
    session.pull_image("docker.io/library/alpine:latest")?;

    let container = session
        .create_container("alpine:latest")?
        .with_name("hello-container")
        .with_init_process(ProcessBuilder::new(["/bin/echo", "Hello from WSL Container!"]))
        .build()?;

    container.start()?;
    std::thread::sleep(std::time::Duration::from_millis(10));
    container.stop(Signal::Terminate, std::time::Duration::from_secs(1))
}

fn main() {
    let _unused = run();
}
```

### Examples

 - `hello_container.rs` a direct port of the end-to-end example on [WSL doc page](https://wsl.dev/api-reference/cpp/end-to-end-example/)
 - `interactive_bash.rs` spawns an ephemeral Ubuntu container that runs bash commands. It also exposes C as a read-only mountpoint and redirect 8080:tcp host port to the container
 - `manipulate_files.rs` an attempt to easily manipulate files inside the container via the API
