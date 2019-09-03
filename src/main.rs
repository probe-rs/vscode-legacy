mod debug_adapter;

use debugserver_types::PauseResponse;
use debugserver_types::PauseArguments;
use debugserver_types::ThreadsResponseBody;
use debugserver_types::ThreadsResponse;
use debugserver_types::Thread;
use debugserver_types::ConfigurationDoneResponse;
use debugserver_types::ConfigurationDoneArguments;
use debugserver_types::SetExceptionBreakpointsResponse;
use debugserver_types::SetExceptionBreakpointsArguments;
use debugserver_types::Breakpoint;
use debugserver_types::SetBreakpointsResponseBody;
use debugserver_types::SetBreakpointsResponse;
use debugserver_types::SetBreakpointsArguments;
use debugserver_types::ProcessEventBody;
use debugserver_types::AttachResponse;
use debugserver_types::DisconnectArguments;
use debugserver_types::LaunchRequestArguments;
use debug_adapter::{DebugAdapter, Event};

use debugserver_types::Request;
use debugserver_types::LaunchResponse;
use std::fs::File;
use std::io;
use std::io::{Read,Write};

use std::env;

use debugserver_types::{InitializeRequest, InitializeResponse, Capabilities};
use serde_json;

use std::path::PathBuf;

use log::debug;
use simplelog::*;


use serde::Deserialize;

fn main() -> std::io::Result<()> {

    if let Ok(path) = env::var("PROBE_RS_LOGFILE") {
        let file = File::create(path)?;

        // Ignore error setting up the debugger
        let _ = WriteLogger::init(LevelFilter::Debug, Config::default(), file);
    }

    let args: Vec<String> = env::args().collect();
    let current_dir = env::current_dir()?;

    debug!("Debugger started in directory {}", current_dir.display());
    debug!("Arguments: {}", args.join(" "));

    let mut adapter = DebugAdapter::new(io::stdin(), io::stdout());

    let data = adapter.receive_data()?;
    let req: InitializeRequest = serde_json::from_slice(&data).unwrap();

    debug!("< {:?}", req);

    let capabilities = Capabilities {
        supports_configuration_done_request: Some(true),
        //supports_function_breakpoints: Some(true),
        ..Default::default()
    };

    let init_resp = InitializeResponse {
        seq: req.seq + 1,
        type_: "response".to_owned(),

        request_seq: req.seq,
        command: "initialize".to_owned(),

        success: true,
        message: None,

        body: Some(capabilities),
    };

    let response_body = serde_json::to_vec(&init_resp).unwrap();

    adapter.send_data(&response_body)?;

    adapter.send_event(&Event::Initialized)?;

    let mut dbg = Debugger::default();
    
    // look for other request
    loop {
        let content = adapter.receive_data()?;

        let req: Request = serde_json::from_slice(&content).unwrap();
        debug!("< {:?}", req);

        match dbg.handle(&mut adapter, &req) {
            HandleResult::Continue => (),
            HandleResult::Stop => { break; },
        }
    }

    debug!("Stopping debugger");

    Ok(())
}

enum HandleResult {
    Continue,
    Stop
}

#[derive(Default, Debug)]
struct Debugger {
    program: Option<PathBuf>,
}

impl Debugger {
    fn handle<R: Read, W: Write>(&mut self, adapter: &mut DebugAdapter<R,W>, req: &Request) -> HandleResult {
        debug!("Handling request {}", req.command);

        match req.command.as_ref() {
            "launch" => {
                let args: LaunchRequestArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);

                // currently, launch is not supported

                let resp = LaunchResponse {
                    command: "launch".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: false,
                    body: None,
                    type_: "response".to_owned(),

                    message: Some("Launching a program is not yet supported.".to_owned()),
                };

                let encoded_resp = serde_json::to_vec(&resp).unwrap();

                adapter.send_data(&encoded_resp).unwrap();
            },
            "attach" => {
                let args: AttachRequestArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);

                self.program = Some(args.program.into());

                let resp = AttachResponse {
                    command: "attach".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    type_: "response".to_owned(),
                    body: None,
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp).unwrap();

                adapter.send_data(&encoded_resp).unwrap();


/*
                // simulate that we attached
                let process_event_data = ProcessEventBody {
                    name: self.program.as_ref().unwrap().to_str().unwrap().to_string(),
                    system_process_id: None,
                    is_local_process: Some(false),
                    start_method: Some("attach".to_owned()),
                    //pointer_size: Some(32), (next version only?)
                };

                let process_event = Event::Process(process_event_data);

                adapter.send_event(&process_event).unwrap();
                */
            },
            "disconnect" => {
                let args: DisconnectArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);
                return HandleResult::Stop;
            },
            "setBreakpoints" => {
                let args: SetBreakpointsArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);


                let mut create_breakpoints = Vec::new();

                for bp in args.breakpoints.as_ref().unwrap() {
                    create_breakpoints.push(Breakpoint{
                        column: bp.column,
                        end_column: None,
                        end_line: None,
                        id: None,
                        line: None,
                        message: None,
                        source: None,
                        verified: true,
                    });
                }

                let breakpoint_body = SetBreakpointsResponseBody {
                    breakpoints: create_breakpoints,
                };

                let resp = SetBreakpointsResponse {
                    command: "setBreakpoints".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    type_: "response".to_owned(),
                    body: breakpoint_body,
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp).unwrap();

                adapter.send_data(&encoded_resp).unwrap();

            },
            "setExceptionBreakpoints" => {
                let args: SetExceptionBreakpointsArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);

                let resp = SetExceptionBreakpointsResponse {
                    command: "setExceptionBreakpoints".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    type_: "response".to_owned(),
                    body: None,
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp).unwrap();

                adapter.send_data(&encoded_resp).unwrap();
            },
            "configurationDone" => {
                //let args: ConfigurationDoneArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                //debug!("Arguments: {:?}", args);

                let resp = ConfigurationDoneResponse {
                    command: "configurationDone".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: None,
                    type_: "response".to_owned(),

                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp).unwrap();

                adapter.send_data(&encoded_resp).unwrap();
            },
            "threads" => {
                //let args: ThreadsArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                //debug!("Arguments: {:?}", args);

                let single_thread = Thread { id: 0, name: "Main Thread".to_owned() };

                let threads = vec![single_thread];

                let resp = ThreadsResponse {
                    command: "threadsDone".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: ThreadsResponseBody{
                        threads,
                    },
                    type_: "response".to_owned(),

                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp).unwrap();

                adapter.send_data(&encoded_resp).unwrap();
            },
            "pause" => {
                let args: PauseArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);


                let resp = PauseResponse {
                    command: "pause".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: None,
                    message: None,
                    type_: "event".to_owned(),
                };

                let encoded_resp = serde_json::to_vec(&resp).unwrap();

                adapter.send_data(&encoded_resp).unwrap();


                // todo: pause execution, and send information back (stopped event)
            },
            _ => unimplemented!(),
        }

        HandleResult::Continue
    }
}


#[derive(Deserialize, Debug)]
struct AttachRequestArguments {
    program: String
}