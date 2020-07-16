use crate::debug_adapter::{self, DebugAdapter};
use probe_rs::{debug::DebugInfo, Probe, Session};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};

use anyhow::anyhow;
use log::{debug, error, info, trace, warn};

use debugserver_types::*;

use debug_adapter::Event;
use serde::{de::DeserializeOwned, Deserialize};

#[derive(Default)]
pub struct Debugger {
    location: PathBuf,
    arguments: AttachRequestArguments,
    program: Option<PathBuf>,
    session: Option<Session>,
    debug_info: Option<DebugInfo>,
    breakpoints: Vec<BreakpointInfo>,
    bp_id: u32,
    current_stackframes: Vec<probe_rs::debug::StackFrame>,
}

impl Debugger {
    pub fn new(location: impl Into<PathBuf>) -> Debugger {
        Debugger {
            location: location.into(),
            ..Default::default()
        }
    }

    fn add_breakpoint(&mut self, bp: &SourceBreakpoint, verified: bool, location: Option<u64>) {
        let id = self.bp_id;
        self.bp_id += 1;

        self.breakpoints.push(BreakpointInfo {
            id,
            verified,
            info: bp.to_owned(),
            address: location,
        });
    }

    pub fn handle<R: Read, W: Write>(
        &mut self,
        adapter: &mut DebugAdapter<R, W>,
        req: &Request,
    ) -> Result<HandleResult, crate::debug_adapter::Error> {
        debug!("Handling request {}", req.command);

        match req.command.as_ref() {
            "launch" => {
                let args: LaunchRequestArguments = get_arguments(req)?;
                trace!("Arguments: {:?}", args);

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

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            }
            "attach" => {
                let args: AttachRequestArguments = get_arguments(req)?;
                trace!("Arguments: {:?}", args);

                let mut program_path = PathBuf::from(args.program.clone());

                if let Some(ref cwd) = args.cwd {
                    debug!("Current working directory: '{}'", cwd);
                    self.location = PathBuf::from(cwd);

                    // If the path to the programm to be debugged is relative, we join if with the
                    if program_path.is_relative() {
                        program_path = self.location.clone().join(&program_path);
                    }
                }

                self.debug_info = match DebugInfo::from_file(&program_path) {
                    Ok(di) => Some(di),
                    Err(e) => {
                        // Just log this, debugging without debug info should be possible.
                        // Showing a warning to the user would be optimal, but not clear how
                        // this can be done with vs code.
                        warn!(
                            "Unable to read debug information from file '{}': {}",
                            program_path.display(),
                            e
                        );
                        None
                    }
                };

                self.program = Some(program_path);

                let session = connect_to_probe(&args.chip);

                self.arguments = args;

                match session {
                    Ok(s) => {
                        self.session = Some(s);

                        info!("Attached to probe");

                        adapter.log_to_console("Attached to probe")?;

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

                        if self.breakpoints.len() > 0 {
                            let mut core = self
                                .session
                                .as_mut()
                                .unwrap()
                                .core(0)
                                .expect("Failed to get core");

                            for bp in self.breakpoints.iter_mut() {
                                if let Some(location) = bp.address {
                                    core.set_hw_breakpoint(location as u32)?;

                                    bp.verified = true;

                                    adapter.send_event(&Event::Breakpoint(bp.get_event_body()))?;
                                }
                            }
                        }
                    }
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
            }
            "disconnect" => {
                let args: DisconnectArguments = get_arguments(req)?;
                trace!("Arguments: {:?}", args);

                let resp = DisconnectResponse {
                    command: "disconnect".to_owned(),
                    request_seq: req.seq,
                    success: true,
                    body: None,
                    seq: adapter.peek_seq(),
                    message: None,
                    type_: "response".to_owned(),
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;

                return Ok(HandleResult::Stop);
            }
            "setBreakpoints" => {
                let args: SetBreakpointsArguments = get_arguments(req)?;

                trace!("Arguments: {:?}", args);

                let mut create_breakpoints = Vec::new();

                let source_path = args.source.path.as_ref().map(Path::new);

                debug!("Source path: {:?}", source_path);

                if let Some(breakpoints) = args.breakpoints.as_ref() {
                    for bp in breakpoints {
                        // Try to find source code location
                        debug!(
                            "Trying to set breakpoint {:?}, source_file {:?}",
                            bp, source_path
                        );

                        let source_location: Option<u64> =
                            self.debug_info.as_ref().and_then(|di| {
                                di.get_breakpoint_location(
                                    dbg!(source_path.unwrap()),
                                    dbg!(bp.line as u64),
                                    bp.column.map(|c| c as u64),
                                )
                                .unwrap_or(None)
                            });

                        if let Some(location) = source_location {
                            debug!("Found source location: {:#08x}!", location);

                            let verified = if let Some(mut core) =
                                self.session.as_mut().and_then(|s| s.core(0).ok())
                            {
                                core.set_hw_breakpoint(location as u32)?;
                                true
                            } else {
                                false
                            };

                            self.add_breakpoint(bp, verified, source_location);

                            create_breakpoints.push(Breakpoint {
                                column: bp.column,
                                end_column: None,
                                end_line: None,
                                id: None,
                                line: Some(bp.line),
                                message: None,
                                source: None,
                                verified,
                            });
                        } else {
                            warn!("Failed to find location for breakpoint {:?}", bp);

                            create_breakpoints.push(Breakpoint {
                                column: bp.column,
                                end_column: None,
                                end_line: None,
                                id: None,
                                line: Some(bp.line),
                                message: None,
                                source: None,
                                verified: false,
                            });
                        }
                    }
                } else {
                    warn!("No breakpoints in request!");
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
            }
            "setExceptionBreakpoints" => {
                let args: SetExceptionBreakpointsArguments = get_arguments(req)?;
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
            }
            "configurationDone" => {
                //let args: ConfigurationDoneArguments = get_arguments(req)?;
                //debug!("Arguments: {:?}", args);

                if self.arguments.reset.unwrap_or(false) {
                    if let Some(mut core) = self.session.as_mut().and_then(|s| s.core(0).ok()) {
                        debug!("Resetting target");
                        core.reset_and_halt(Duration::from_millis(10))?;

                        if !self.arguments.halt_after_reset.unwrap_or(true) {
                            core.run()?;
                        }
                    }
                }

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
            }
            "threads" => {
                //let args: ThreadsArguments = serde_json::from_value(req.arguments.as_ref().unwrap().clone()).unwrap();
                //debug!("Arguments: {:?}", args);

                let single_thread = Thread {
                    id: 0,
                    name: "Main Thread".to_owned(),
                };

                let threads = vec![single_thread];

                let resp = ThreadsResponse {
                    command: "threads".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: ThreadsResponseBody { threads },
                    type_: "response".to_owned(),

                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            }
            "pause" => {
                let args: PauseArguments = get_arguments(req)?;
                debug!("Arguments: {:?}", args);

                let resp = PauseResponse {
                    command: "pause".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: None,
                    message: None,
                    type_: "response".to_owned(),
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

                        debug!("Sended stopped event");
                    }
                    Err(e) => {
                        warn!("Error trying to pause target: {:?}", e);
                    }
                }
            }
            "stackTrace" => {
                let args: StackTraceArguments = get_arguments(req)?;
                debug!("Arguments: {:?}", args);

                let mut core = self.session.as_mut().and_then(|s| s.core(0).ok()).unwrap();

                let regs = core.registers();

                let pc = core.read_core_reg(regs.program_counter()).unwrap();
                debug!("Stopped at address 0x{:08x}", pc);

                if let Some(debug_info) = self.debug_info.as_ref() {
                    self.current_stackframes =
                        debug_info.try_unwind(&mut core, pc as u64).collect();

                    let frame_list: Vec<StackFrame> = self
                        .current_stackframes
                        .iter()
                        .map(|f| {
                            use probe_rs::debug::ColumnType::*;

                            let column = f
                                .source_location
                                .as_ref()
                                .and_then(|sl| {
                                    sl.column.map(|col| match col {
                                        LeftEdge => 0,
                                        Column(c) => c,
                                    })
                                })
                                .unwrap_or(0);

                            let sl = f.source_location.as_ref().unwrap();

                            let mut path: PathBuf = sl.directory.as_ref().unwrap().into();

                            path.push(sl.file.as_ref().unwrap());

                            let source = Some(Source {
                                name: Some(sl.file.clone().unwrap()),
                                path: path.to_str().map(|s| s.to_owned()),
                                source_reference: None,
                                presentation_hint: None,
                                origin: None,
                                sources: None,
                                adapter_data: None,
                                checksums: None,
                            });

                            let line = f
                                .source_location
                                .as_ref()
                                .and_then(|sl| sl.line)
                                .unwrap_or(0) as i64;

                            debug!(
                                "  Frame {: <2} - {}:{}:{}",
                                f.id,
                                path.display(),
                                line,
                                column
                            );

                            StackFrame {
                                id: f.id as i64,
                                name: f.function_name.clone(),
                                source: source,
                                line: line,
                                column: column as i64,
                                end_column: None,
                                end_line: None,
                                module_id: None,
                                presentation_hint: Some("normal".to_owned()),
                            }
                        })
                        .collect();

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
                } else {
                    // No debug information, so we cannot send stack trace information
                    let body = StackTraceResponseBody {
                        stack_frames: vec![],
                        total_frames: None,
                    };

                    let resp = StackTraceResponse {
                        command: "stackTrace".to_owned(),
                        request_seq: req.seq,
                        seq: adapter.peek_seq(),
                        body: body,
                        success: false,
                        message: Some("No debug information found".to_owned()),
                        type_: "response".to_owned(),
                    };

                    let encoded_resp = serde_json::to_vec(&resp)?;

                    adapter.send_data(&encoded_resp)?;
                }
            }
            "scopes" => {
                let args: ScopesArguments = get_arguments(req)?;
                debug!("Arguments: {:?}", args);

                let mut scopes = vec![];

                if let Some(frame) = self
                    .current_stackframes
                    .iter()
                    .find(|sf| sf.id == args.frame_id as u64)
                {
                    use probe_rs::debug::ColumnType::*;

                    let sl = frame.source_location.as_ref().unwrap();
                    let path: PathBuf = sl.directory.as_ref().unwrap().into();

                    let source = Some(Source {
                        name: Some(sl.file.clone().unwrap()),
                        path: path.to_str().map(|s| s.to_owned()),
                        source_reference: None,
                        presentation_hint: None,
                        origin: None,
                        sources: None,
                        adapter_data: None,
                        checksums: None,
                    });

                    let scope = Scope {
                        line: frame
                            .source_location
                            .as_ref()
                            .and_then(|l| l.line.map(|l| l as i64)),
                        column: frame.source_location.as_ref().and_then(|l| {
                            l.column.map(|c| match c {
                                LeftEdge => 0,
                                Column(c) => c as i64,
                            })
                        }),
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
                    body: ScopesResponseBody { scopes },
                    type_: "response".to_owned(),
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            }
            "source" => {
                let args: SourceArguments = get_arguments(req)?;
                debug!("Arguments: {:?}", args);

                let resp = if let Some(path) = args.source.and_then(|s| s.path) {
                    let mut source_path = PathBuf::from(path);

                    if source_path.is_relative() {
                        source_path = self.location.join(source_path);
                    }

                    match std::fs::read_to_string(source_path) {
                        Ok(content) => SourceResponse {
                            type_: "response".to_owned(),
                            command: "source".to_owned(),
                            request_seq: req.seq,
                            seq: adapter.peek_seq(),
                            message: None,
                            success: true,
                            body: SourceResponseBody {
                                content,
                                mime_type: None,
                            },
                        },
                        Err(e) => SourceResponse {
                            type_: "response".to_owned(),
                            command: "source".to_owned(),
                            request_seq: req.seq,
                            seq: adapter.peek_seq(),
                            message: None,
                            success: false,
                            body: SourceResponseBody {
                                content: format!("Unable to open resource: {}", e),
                                mime_type: None,
                            },
                        },
                    }
                } else {
                    SourceResponse {
                        type_: "response".to_owned(),
                        command: "source".to_owned(),
                        request_seq: req.seq,
                        seq: adapter.peek_seq(),
                        message: None,
                        success: false,
                        body: SourceResponseBody {
                            content: "Unable to open resource".to_owned(),
                            mime_type: None,
                        },
                    }
                };

                let encoded_resp = serde_json::to_vec(&resp)?;
                adapter.send_data(&encoded_resp)?;
            }
            "variables" => {
                let args: VariablesArguments = get_arguments(req)?;
                debug!("Arguments: {:?}", args);

                let mut variables = vec![];

                if let Some(frame) = self
                    .current_stackframes
                    .iter()
                    .find(|sf| sf.id == args.variables_reference as u64)
                {
                    variables = frame
                        .variables
                        .iter()
                        .map(|variable| Variable {
                            name: variable.name.clone(),
                            value: variable.value.to_string(),
                            type_: None,
                            presentation_hint: None,
                            evaluate_name: None,
                            variables_reference: -1,
                            named_variables: None,
                            indexed_variables: None,
                        })
                        .collect();
                    debug!("{:?}", &variables);
                }

                let resp = VariablesResponse {
                    command: "variables".to_owned(),
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    success: true,
                    body: VariablesResponseBody { variables },
                    type_: "response".to_owned(),
                    message: None,
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;
            }
            "continue" => {
                let args: ContinueArguments = get_arguments(req)?;
                debug!("Arguments: {:?}", args);

                if let Some(ref mut core) = self.session.as_mut().and_then(|s| s.core(0).ok()) {
                    core.run().expect("Failed to continue running target.");
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
            }
            "next" => {
                let args: NextArguments = get_arguments(req)?;
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

                if let Some(ref mut core) = self.session.as_mut().and_then(|s| s.core(0).ok()) {
                    let _cpu_info = core.step().expect("Failed to continue running target.");

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
            }
            cmd => {
                error!(
                    "Received request {}, which is not supported / implemented yet",
                    cmd
                );

                let resp = ErrorResponse {
                    command: cmd.to_owned(),
                    success: false,
                    request_seq: req.seq,
                    seq: adapter.peek_seq(),
                    type_: "response".to_owned(),

                    body: ErrorResponseBody {
                        error: Some(Message {
                            id: 1,
                            send_telemetry: Some(false),
                            format: "This type of request is not yet supported.".to_owned(),
                            variables: None,
                            show_user: Some(true),
                            url: None,
                            url_label: None,
                        }),
                    },
                    message: Some("cancelled".to_owned()),
                };

                let encoded_resp = serde_json::to_vec(&resp)?;

                adapter.send_data(&encoded_resp)?;

                adapter.log_to_console(format!("Received unsupported request '{}'\n", cmd))?;
            }
        }

        Ok(HandleResult::Continue)
    }

    fn pause(&mut self) -> Result<bool, probe_rs::Error> {
        match self.session.as_mut().and_then(|s| s.core(0).ok()) {
            Some(ref mut core) => {
                debug!("Trying to pause target");
                let cpi = core.halt(Duration::from_millis(10))?;
                debug!("Paused target at pc=0x{:08x}", cpi.pc);

                Ok(true)
            }
            None => Ok(false),
        }
    }
}

#[derive(Deserialize, Debug, Default)]
struct AttachRequestArguments {
    program: String,
    chip: String,
    cwd: Option<String>,
    reset: Option<bool>,
    halt_after_reset: Option<bool>,
}

pub fn get_arguments<T: DeserializeOwned>(req: &Request) -> Result<T, debug_adapter::Error> {
    let value = req
        .arguments
        .as_ref()
        .ok_or(debug_adapter::Error::InvalidRequest)?;

    serde_json::from_value(value.to_owned()).map_err(|e| e.into())
}

fn connect_to_probe(target: &str) -> Result<Session, anyhow::Error> {
    let mut probes = Probe::list_all();

    let probe_info = probes.pop().ok_or(anyhow!("Failed to find probe"))?;

    let probe = probe_info.open()?;

    let session = probe.attach(target)?;

    Ok(session)
}

pub enum HandleResult {
    Continue,
    Stop,
}

#[derive(Debug)]
struct BreakpointInfo {
    id: u32,
    verified: bool,
    info: SourceBreakpoint,
    address: Option<u64>,
}

impl BreakpointInfo {
    fn get_event_body(&self) -> BreakpointEventBody {
        BreakpointEventBody {
            reason: "changed".to_owned(),
            breakpoint: Breakpoint {
                id: Some(self.id as i64),
                column: self.info.column,
                end_column: None,
                line: Some(self.info.line),
                end_line: None,
                message: None,
                verified: self.verified,
                source: None,
            },
        }
    }
}
