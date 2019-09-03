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

    pub fn receive_data(&mut self) -> Result<Vec<u8>, io::Error> {
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

    pub fn send_data(&mut self, raw_data: &[u8]) -> Result<(), io::Error> {
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

    pub fn send_event(&mut self, event: &Event) -> Result<(), io::Error> {
        let body = event.serialize(self.seq).unwrap();

        self.send_data(&body)
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
    Output,
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
    fn serialize(&self, seq: i64) -> serde_json::Result<Vec<u8>> {
        use Event::*;

        match self {
            Initialized => serde_json::to_vec(&InitializedEvent {
                seq,
                body: None,
                type_: "event".to_owned(),
                event: "initialized".to_owned(),
            }),
            Process(ref body) => serde_json::to_vec(&ProcessEvent{
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "process".to_owned(),
            }),
            Thread(ref body) => serde_json::to_vec(&ThreadEvent{
                seq,
                body: body.clone(),
                type_: "event".to_owned(),
                event: "thread".to_owned(),
            }),
            _ => unimplemented!()
        }
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