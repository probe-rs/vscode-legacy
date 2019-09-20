use debugserver_types::OutputEvent;
use debugserver_types::OutputEventBody;
use debugserver_types::StoppedEvent;
use debugserver_types::ThreadEvent;
use debugserver_types::ThreadEventBody;
use debugserver_types::ProcessEvent;
use debugserver_types::ProcessEventBody;
use log::debug;

use std::io;
use std::io::{Read, Write};
use std::io::{BufRead, BufReader};

use std::str;

use debugserver_types::{
    InitializedEvent,
    StoppedEventBody
};

#[derive(Debug)]
pub enum Error {
    IoError(io::Error),
    SerdeError(serde_json::Error),
    Unimplemented,
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<serde_json::Error> for Error {
    fn from(e: serde_json::Error) -> Self {
        Error::SerdeError(e)
    }
}

pub struct DebugAdapter<R: Read, W: Write> {
    seq: i64,
    input: BufReader<R>,
    output: W,
}

impl<R: Read, W: Write> DebugAdapter<R,W> {
    pub fn new(input: R, output: W) -> DebugAdapter<R,W> {
        DebugAdapter {
            seq: 1,
            input: BufReader::new(input),
            output
        }
    }

    pub fn peek_seq(&self) -> i64 {
        self.seq
    }

    pub fn receive_data(&mut self) -> Result<Vec<u8>, Error> {
        let mut header = String::new();

        self.input.read_line(&mut header)?;
        debug!("< {}", header.trim_end());

        // we should read an empty line here
        let mut buff = String::new();
        self.input.read_line(&mut buff)?;

        let len = get_content_len(&header).unwrap();

        let mut content = vec![0u8; len];
        let bytes_read = self.input.read(&mut content)?;

        assert!(bytes_read == len);

        Ok(content)

    }

    pub fn send_data(&mut self, raw_data: &[u8]) -> Result<(), Error> {
        let response_body = raw_data;

        let response_header = format!("Content-Length: {}\r\n\r\n", response_body.len());

        debug!("> {}", response_header.trim_end());
        debug!("> {}", str::from_utf8(response_body).unwrap());

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
    Breakpoint,
    Terminated(RestartRequest),
    Initialized,
    Capabilities,
    LoadedSource
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
            Process(ref body) => serde_json::to_vec(&ProcessEvent{
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "process".to_owned(),
            })?,
            Thread(ref body) => serde_json::to_vec(&ThreadEvent{
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "thread".to_owned(),
            })?,
            Stopped(ref body) => serde_json::to_vec(&StoppedEvent{
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "stopped".to_owned(),
            })?,
            Output(ref body) => serde_json::to_vec(&OutputEvent{
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "output".to_owned(),
            })?,
            _ => return Err(Error::Unimplemented),
        };

        Ok(data)
    }

    pub fn console_output(msg: String) -> Event {
        Event::Output(
            OutputEventBody {
                output: msg,
                category: Some("console".to_owned()),
                variables_reference: None,
                source: None,
                line: None,
                column: None,
                data: None, 
            }
        )
    }
}

#[derive(Debug)]
pub enum RestartRequest {
    Yes,
    No
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