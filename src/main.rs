mod debug_adapter;

use probe::debug_probe::DebugProbe;
use probe::debug_probe::DebugProbeError;
use debugserver_types::*;
use debug_adapter::{DebugAdapter, Event};

use daplink;

use probe::debug_probe::MasterProbe;
use probe;
use probe::target::Target;

use probe_rs_debug::session::Session;
use probe_rs_debug::debug::DebugInfo;

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
use log::info;
use log::warn;
use log::error;

use simplelog::*;


use serde::Deserialize;

fn main() -> Result<(), debug_adapter::Error> {

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
    let req: InitializeRequest = serde_json::from_slice(&data)?;

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

    let response_body = serde_json::to_vec(&init_resp)?;

    adapter.send_data(&response_body)?;

    adapter.send_event(&Event::Initialized)?;

    let mut dbg = Debugger::default();
    
    // look for other request
    loop {
        let content = adapter.receive_data()?;
        debug!("< {:?}", content);

        let req: Request = serde_json::from_slice(&content)?;
        debug!("< {:?}", req);

        match dbg.handle(&mut adapter, &req) {
            Ok(r) => match r {
                HandleResult::Continue => (),
                HandleResult::Stop => { break; },
            },
            Err(e) => {
                error!("Failed to handle request from debug client: {:?}", e);
                break;
            }
        }
    }

    debug!("Stopping debugger");

    Ok(())
}

enum HandleResult {
    Continue,
    Stop
}

#[derive(Default)]
struct Debugger {
    program: Option<PathBuf>,
    session: Option<Session>,
    debug_info: Option<DebugInfo>,
    current_stackframes: Vec<probe_rs_debug::debug::StackFrame>,
}

impl Debugger {

    fn handle<R: Read, W: Write>(&mut self, adapter: &mut DebugAdapter<R,W>, req: &Request) -> Result<HandleResult, debug_adapter::Error> {
        debug!("Handling request {}", req.command);

        match req.command.as_ref() {
            "launch" => {
                let args: LaunchRequestArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone())?;
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

                adapter.send_data(&encoded_resp)?;
            },
            "attach" => {
                let args: AttachRequestArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone())?;
                debug!("Arguments: {:?}", args);


                let debug_data = match std::fs::File::open(&args.program) {
                    Ok(file) =>  unsafe { memmap::Mmap::map(&file).ok() },
                    Err(e) => {
                        debug!("Failed to open program file: {:?}", e);
                        None
                    },
                };
                                               
                self.program = Some(args.program.into());

                self.debug_info = debug_data.map(|mmap| DebugInfo::from_raw(&*mmap));

                let session = connect_to_probe();

                match session {
                    Ok(s) => {
                        self.session = Some(s);

                        info!("Attached to probe");

                        let resp = AttachResponse {
                            command: "attach".to_owned(),
                            request_seq: req.seq,
                            seq: adapter.peek_seq(),
                            success: true,
                            type_: "response".to_owned(),
                            body: None,
                            message: None,
                        };

                        let encoded_resp = serde_json::to_vec(&resp)?;

                        adapter.send_data(&encoded_resp)?;
                    },
                    Err(e) => {
                        warn!("Failed to attacht to probe: {:?}", e);
                        
                        let resp = AttachResponse {
                            command: "attach".to_owned(),
                            request_seq: req.seq,
                            seq: adapter.peek_seq(),
                            success: false,
                            type_: "response".to_owned(),
                            body: None,
                            message: Some("Failed to attach to probe.".to_owned()),
                        };

                        let encoded_resp = serde_json::to_vec(&resp)?;

                        adapter.send_data(&encoded_resp)?;
                    }
                }
            },
            "disconnect" => {
                let args: DisconnectArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone())?;
                debug!("Arguments: {:?}", args);
                return Ok(HandleResult::Stop);
            },
            "setBreakpoints" => {
                let args: SetBreakpointsArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone())?;
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

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;

            },
            "setExceptionBreakpoints" => {
                let args: SetExceptionBreakpointsArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone())?;
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

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
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

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            },
            "threads" => {
                //let args: ThreadsArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                //debug!("Arguments: {:?}", args);

                let single_thread = Thread { id: 0, name: "Main Thread".to_owned() };

                let threads = vec![single_thread];

                let resp = ThreadsResponse {
                    command: "threads".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: ThreadsResponseBody{
                        threads,
                    },
                    type_: "response".to_owned(),

                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            },
            "pause" => {
                let args: PauseArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone())?;
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

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;

                match self.pause() {
                    Ok(_) => {
                        debug!("Stopped, sending pause event");

                        let event_body = StoppedEventBody {
                            reason: "pause".to_owned(),
                            description: Some("Target paused due to pause request.".to_owned()),
                            thread_id: Some(0),
                            preserve_focus_hint: None,
                            text: None,
                            all_threads_stopped: None,
                        };
                        adapter.send_event(&Event::Stopped(event_body))?;

                        debug!("Sended pause event");
                    },
                    Err(e) => {
                        warn!("Error trying to pause target: {:?}", e);
                    }
                } 

            },
            "stackTrace" => {
                let args: StackTraceArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone())?;
                debug!("Arguments: {:?}", args);

                let session = self.session.as_mut().unwrap();

                use probe::target::m0::PC;

                let pc = session.target.read_core_reg(&mut session.probe, PC).unwrap();
                debug!("Stopped at address 0x{:08x}", pc);

                let debug_info = self.debug_info.as_ref().unwrap();


                self.current_stackframes = debug_info.try_unwind(session, pc as u64).collect();

                debug!("Got stacktraces...");

                let frame_list: Vec<StackFrame> = self.current_stackframes.iter().map(|f| {

                    use probe_rs_debug::debug::ColumnType::*;

                    let column = f.source_location.as_ref().and_then( |sl| sl.column.map(|col| { match col {
                        LeftEdge => 0,
                        Column(c) => c,
                    }})).unwrap_or(0);

                    let sl = f.source_location.as_ref().unwrap();

                    let mut path: PathBuf = sl.directory.as_ref().unwrap().into();

                    path.push(sl.file.as_ref().unwrap());


                    let source = Some(
                        Source{
                            name: Some(sl.file.clone().unwrap()),
                            path: path.to_str().map(|s| s.to_owned()),
                            source_reference: None,
                            presentation_hint: None,
                            origin: None,
                            sources: None,
                            adapter_data: None,
                            checksums: None,
                        }
                    );

                    StackFrame {
                        id: f.id as i64,
                        name: f.function_name.clone(),
                        source: source,
                        line: f.source_location.as_ref().and_then( |sl| sl.line ).unwrap_or(0) as i64,
                        column: column as i64,
                        end_column: None,
                        end_line: None,
                        module_id: None,
                        presentation_hint: None,
                    }
                }).collect();

                let frame_len = frame_list.len();

                let body = StackTraceResponseBody {
                    stack_frames: frame_list,
                    total_frames: Some(frame_len as i64),
                };

                let resp = StackTraceResponse {
                    command: "stackTrace".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body,
                    message: None,
                    type_: "response".to_owned(),
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            },
            "scopes" => {
                let args: ScopesArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);

                let mut scopes = vec![];

                if let Some(frame) = self.current_stackframes.iter().find(|sf| sf.id == args.frame_id as u64) {
                    use probe_rs_debug::debug::ColumnType::*;

                    let sl = frame.source_location.as_ref().unwrap();
                    let mut path: PathBuf = sl.directory.as_ref().unwrap().into();

                    let source = Some(
                        Source{
                            name: Some(sl.file.clone().unwrap()),
                            path: path.to_str().map(|s| s.to_owned()),
                            source_reference: None,
                            presentation_hint: None,
                            origin: None,
                            sources: None,
                            adapter_data: None,
                            checksums: None,
                        }
                    );

                    let scope = Scope {
                        line: frame.source_location.as_ref().and_then(|l| l.line.map(|l| l as i64)),
                        column: frame.source_location.as_ref().and_then(|l| l.column.map(|c| match c {
                            LeftEdge => 0,
                            Column(c) => c as i64,
                        })),
                        end_column: None,
                        end_line: None,
                        expensive: false,
                        indexed_variables: None,
                        name: "Locals".to_string(),
                        named_variables: None,
                        source: source,
                        variables_reference: frame.id as i64,
                    };

                    scopes.push(scope);
                }

                let resp = ScopesResponse {
                    command: "scopes".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: ScopesResponseBody {
                        scopes,
                    },
                    type_: "response".to_owned(),
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            },
            "variables" => {
                let args: VariablesArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);

                let mut variables = vec![];

                if let Some(frame) = self.current_stackframes.iter().find(|sf| sf.id == args.variables_reference as u64) {
                    variables = frame
                        .variables
                        .iter()
                        .map(|variable| {
                        Variable {
                            name: variable.name.clone(),
                            value: variable.value.to_string(),
                            type_: None,
                            presentation_hint: None,
                            evaluate_name: None,
                            variables_reference: args.variables_reference,
                            named_variables: None,
                            indexed_variables: None,
                        }
                    }).collect();
                    debug!("{:?}", &variables);
                }

                let resp = VariablesResponse {
                    command: "variables".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: VariablesResponseBody {
                        variables,
                    },
                    type_: "response".to_owned(),
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            },
            "continue" => {
                let args: ContinueArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);

                if let Some(ref mut session) = self.session {
                    session.target.run(&mut session.probe).expect("Failed to continue running target.");
                }

                let resp = ContinueResponse {
                    command: "continue".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: ContinueResponseBody {
                        all_threads_continued: Some(true),
                    },
                    type_: "response".to_owned(),
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            },
            "next" => {
                let args: NextArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                debug!("Arguments: {:?}", args);

                let resp = NextResponse {
                    command: "next".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: None,
                    type_: "response".to_owned(),
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;

                if let Some(ref mut session) = self.session {
                    let _cpu_info = session.target.step(&mut session.probe).expect("Failed to continue running target.");

                    debug!("Stopped, sending pause event");

                    let event_body = StoppedEventBody {
                        reason: "step".to_owned(),
                        description: Some("Target paused after step.".to_owned()),
                        thread_id: Some(0),
                        preserve_focus_hint: None,
                        text: None,
                        all_threads_stopped: None,
                    };
                    adapter.send_event(&Event::Stopped(event_body))?;

                    debug!("Sended pause event");

                }

            },
            _ => unimplemented!(),
        }

        Ok(HandleResult::Continue)
    }

    fn pause(&mut self) -> Result<bool, DebugProbeError> {
        match self.session {
            Some(ref mut s) => {
                debug!("Trying to pause target");
                let cpi = s.target.halt(&mut s.probe)?;
                debug!("Paused target at pc=0x{:08x}", cpi.pc);

                Ok(true)
            },
            None => {
                Ok(false)
            }
        }
    }
}


#[derive(Deserialize, Debug)]
struct AttachRequestArguments {
    program: String
}


fn connect_to_probe() -> Result<Session, DebugProbeError> {
    let device = daplink::tools::list_daplink_devices().pop().ok_or(DebugProbeError::ProbeCouldNotBeCreated)?;


    let mut link = daplink::DAPLink::new_from_probe_info(&device)?;

    link.attach(Some(probe::protocol::WireProtocol::Swd))?;
    
    let probe = MasterProbe::from_specific_probe(link);

    let target = probe::target::m0::M0;
    
    Ok(Session::new(Box::new(target), probe))
}
