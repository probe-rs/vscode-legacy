mod debug_adapter;
mod debugger;

use debug_adapter::{DebugAdapter, DebugAdapterMessage, Event};

use debugserver_types::InitializeRequestArguments;

use std::{
    env,
    fs::File,
    io,
    io::{Read, Write},
    net::{SocketAddr, TcpListener},
    path::PathBuf,
};

use debugserver_types::Capabilities;
use log::{debug, error, info, trace};

use simplelog::*;

use clap::{App, Arg};

use debugger::{get_arguments, Debugger, HandleResult};

use anyhow::anyhow;

fn main() -> Result<(), anyhow::Error> {
    let matches = App::new("probe-rs - Debug Adapter for vscode")
        .arg(Arg::with_name("server").long("server"))
        .arg(
            Arg::with_name("port")
                .short("p")
                .long("port")
                .takes_value(true)
                .max_values(1),
        )
        .get_matches();

    let current_dir = env::current_dir()?;

    let cfg = ConfigBuilder::new()
        //.add_filter_allow_str("probe_rs_debugadapter")
        .build();

    let log_level = LevelFilter::Debug;

    if matches.is_present("server") {
        // Setup terminal logger
        let _ = TermLogger::init(log_level, cfg, TerminalMode::Mixed);

        let port: u16 = matches
            .value_of("port")
            .map(|s| u16::from_str_radix(s, 10).unwrap())
            .unwrap_or(8000);
        info!("Starting in server mode on port {}", port);

        let addr = SocketAddr::from(([127, 0, 0, 1], port));

        let listener = TcpListener::bind(addr)?;

        let (socket, addr) = listener.accept()?;

        info!("Accepted connection from {}", addr);

        let reader = socket.try_clone()?;
        let writer = socket;

        let adapter = DebugAdapter::new(reader, writer);

        run(adapter, &current_dir)
    } else {
        if let Ok(path) = env::var("PROBE_RS_LOGFILE") {
            let file = File::create(path)?;

            // Ignore error setting up the debugger
            let _ = WriteLogger::init(log_level, cfg, file);
        }
        debug!("Debugger started in directory {}", current_dir.display());

        let adapter = DebugAdapter::new(io::stdin(), io::stdout());

        run(adapter, &current_dir)
    }
}

fn run<R: Read, W: Write>(
    mut adapter: DebugAdapter<R, W>,
    cwd: &PathBuf,
) -> Result<(), anyhow::Error> {
    let data = adapter.receive_data()?;

    let request = match data {
        DebugAdapterMessage::Request(request) => request,
        _ => return Err(anyhow!("Expected request as initial message")),
    };

    if request.command != "initialize" {
        return Err(anyhow!(
            "Initial command was '{}', expected 'initialize'",
            request.command
        ));
    }

    let arguments: InitializeRequestArguments = get_arguments(&request)?;

    debug!(
        "Initialization request from client '{}'",
        arguments.client_name.unwrap_or("<unknown>".to_owned())
    );

    let capabilities = Capabilities {
        supports_configuration_done_request: Some(true),
        //supports_function_breakpoints: Some(true),
        ..Default::default()
    };

    adapter.send_response(&request, Ok(Some(capabilities)))?;

    adapter.send_event(&Event::Initialized)?;

    let mut dbg = Debugger::new(cwd);

    // look for other request
    loop {
        let message = adapter.receive_data()?;
        trace!("< {:?}", message);

        match dbg.handle(&mut adapter, &message) {
            Ok(r) => match r {
                HandleResult::Continue => (),
                HandleResult::Stop => {
                    break;
                }
            },
            Err(e) => {
                error!("Failed to handle request from debug client: {:?}", e);
                break;
            }
        }
    }

    adapter.send_event(&Event::Terminated(debug_adapter::RestartRequest::No))?;

    debug!("Stopping debugger");

    Ok(())
}
