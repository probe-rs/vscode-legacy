use debugserver_types::BreakpointEvent;
use debugserver_types::BreakpointEventBody;
use debugserver_types::OutputEvent;
use debugserver_types::OutputEventBody;
use debugserver_types::ProcessEvent;
use debugserver_types::ProcessEventBody;
use debugserver_types::ProtocolMessage;
use debugserver_types::Request;
use debugserver_types::Response;
use debugserver_types::StoppedEvent;
use debugserver_types::TerminatedEvent;
use debugserver_types::TerminatedEventBody;
use debugserver_types::ThreadEvent;
use debugserver_types::ThreadEventBody;
use log::trace;

use std::io;
use std::io::{BufRead, BufReader};
use std::io::{Read, Write};

use std::str;
use std::string::ToString;

use debugserver_types::{InitializedEvent, StoppedEventBody};

use anyhow::{anyhow, Result};
use serde::Serialize;
use thiserror;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Input error")]
    IoError(#[from] io::Error),
    #[error("Serialiation error")]
    SerdeError(#[from] serde_json::Error),
    #[error("Error in interaction with probe")]
    ProbeError(#[from] probe_rs::Error),
    #[error("Missing session for interaction with probe")]
    MissingSession,
    #[error("Received an invalid requeset")]
    InvalidRequest,
    #[error("Request not implemented")]
    Unimplemented,
    #[error(transparent)]
    Other(#[from] anyhow::Error),
}

pub struct DebugAdapter<R: Read, W: Write> {
    seq: i64,
    input: BufReader<R>,
    output: W,
}

#[derive(Debug)]
pub enum DebugAdapterMessage {
    Request(Request),
    Response(Response),
    Event(debugserver_types::Event),
}

impl<R: Read, W: Write> DebugAdapter<R, W> {
    pub fn new(input: R, output: W) -> DebugAdapter<R, W> {
        DebugAdapter {
            seq: 1,
            input: BufReader::new(input),
            output,
        }
    }

    pub fn peek_seq(&self) -> i64 {
        self.seq
    }

    pub fn receive_data(&mut self) -> Result<DebugAdapterMessage> {
        let mut header = String::new();

        self.input.read_line(&mut header)?;
        trace!("< {}", header.trim_end());

        // we should read an empty line here
        let mut buff = String::new();
        self.input.read_line(&mut buff)?;

        let len = get_content_len(&header)
            .ok_or_else(|| anyhow!("Failed to read content length from header '{}'", header))?;

        let mut content = vec![0u8; len];
        let bytes_read = self.input.read(&mut content)?;

        assert!(bytes_read == len);

        // Extract protocol message
        let protocol_message: ProtocolMessage = serde_json::from_slice(&content)?;

        match protocol_message.type_.as_ref() {
            "request" => Ok(DebugAdapterMessage::Request(serde_json::from_slice(
                &content,
            )?)),
            "response" => Ok(DebugAdapterMessage::Response(serde_json::from_slice(
                &content,
            )?)),
            "event" => Ok(DebugAdapterMessage::Event(serde_json::from_slice(
                &content,
            )?)),
            other => Err(anyhow!("Unknown message type: {}", other)),
        }
    }

    pub fn send_response<S: Serialize>(
        &mut self,
        request: &Request,
        response: Result<Option<S>, Error>,
    ) -> Result<(), Error> {
        let mut resp = Response {
            command: request.command.clone(),
            request_seq: request.seq,
            seq: self.peek_seq(),
            success: false,
            body: None,
            type_: "response".to_owned(),
            message: None,
        };

        match response {
            Ok(value) => {
                let body_value = match value {
                    Some(value) => Some(serde_json::to_value(value)?),
                    None => None,
                };
                resp.success = true;
                resp.body = body_value;
            }
            Err(e) => {
                resp.success = false;
                resp.message = Some(e.to_string());
            }
        };

        let encoded_resp = serde_json::to_vec(&resp)?;

        self.send_data(&encoded_resp)
    }

    fn send_data(&mut self, raw_data: &[u8]) -> Result<(), Error> {
        let response_body = raw_data;

        let response_header = format!("Content-Length: {}\r\n\r\n", response_body.len());

        trace!("> {}", response_header.trim_end());
        trace!("> {}", str::from_utf8(response_body).unwrap());

        self.output.write(response_header.as_bytes())?;
        self.output.write(response_body)?;

        self.output.flush()?;

        self.seq += 1;

        Ok(())
    }

    pub fn send_event(&mut self, event: &Event) -> Result<(), Error> {
        let body = event.serialize(self.seq)?;

        self.send_data(&body)
    }

    pub fn log_to_console<S: Into<String>>(&mut self, msg: S) -> Result<(), Error> {
        let output_event = Event::console_output(msg.into());
        self.send_event(&output_event)?;

        Ok(())
    }
}

fn get_content_len(header: &str) -> Option<usize> {
    let mut parts = header.trim_end().split_ascii_whitespace();

    // discard first part
    parts.next()?;

    parts.next()?.parse::<usize>().ok()
}

#[derive(Debug)]
pub enum Event {
    Exited(i64),
    Module,
    Output(OutputEventBody),
    Thread(ThreadEventBody),
    Process(ProcessEventBody),
    Stopped(StoppedEventBody),
    Continued,
    Breakpoint(BreakpointEventBody),
    Terminated(RestartRequest),
    Initialized,
    Capabilities,
    LoadedSource,
}

impl Event {
    fn serialize(&self, seq: i64) -> Result<Vec<u8>, Error> {
        use Event::*;

        let data = match self {
            Initialized => serde_json::to_vec(&InitializedEvent {
                seq,
                body: None,
                type_: "event".to_owned(),
                event: "initialized".to_owned(),
            })?,
            Process(ref body) => serde_json::to_vec(&ProcessEvent {
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "process".to_owned(),
            })?,
            Thread(ref body) => serde_json::to_vec(&ThreadEvent {
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "thread".to_owned(),
            })?,
            Stopped(ref body) => serde_json::to_vec(&StoppedEvent {
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "stopped".to_owned(),
            })?,
            Output(ref body) => serde_json::to_vec(&OutputEvent {
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "output".to_owned(),
            })?,
            Terminated(restart_request) => {
                let body = TerminatedEventBody {
                    restart: Some(serde_json::Value::Bool(
                        restart_request == &RestartRequest::Yes,
                    )),
                };

                serde_json::to_vec(&TerminatedEvent {
                    seq,
                    body: Some(body),
                    type_: "event".to_owned(),
                    event: "terminated".to_owned(),
                })?
            }
            Breakpoint(ref body) => serde_json::to_vec(&BreakpointEvent {
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "breakpoint".to_owned(),
            })?,
            _ => return Err(Error::Unimplemented),
        };

        Ok(data)
    }

    pub fn console_output(msg: String) -> Event {
        Event::Output(OutputEventBody {
            output: msg,
            category: Some("console".to_owned()),
            variables_reference: None,
            source: None,
            line: None,
            column: None,
            data: None,
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq)]
pub enum RestartRequest {
    Yes,
    No,
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn parse_valid_header() {
        let header = "Content-Length: 234\r\n";

        assert_eq!(234, get_content_len(&header).unwrap());
    }
}
