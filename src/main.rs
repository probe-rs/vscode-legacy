use debugserver_types::InitializedEvent;
use debugserver_types::Request;
use std::fs::File;
use std::io;
use std::io::{Read, Write};

use std::env;

use debugserver_types::{InitializeRequest, InitializeResponse, Capabilities};
use serde_json;

use log::debug;
use simplelog::*;


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

    // Simulate staying open, to see if something interesting happens
    let mut header = String::new();

    io::stdin().read_line(&mut header)?;
    debug!("< {}", header.trim_end());

    // we should read an empty line here
    let mut buff = String::new();
    io::stdin().read_line(&mut buff)?;

    let len = get_content_len(&header);

    let mut content = vec![0u8; len];
    let bytes_read = io::stdin().read(&mut content)?;

    assert!(bytes_read == len);

    let req: InitializeRequest = serde_json::from_slice(&content).unwrap();
    debug!("< {:?}", req);

    let init_resp = InitializeResponse {
        seq: req.seq + 1,
        type_: "response".to_owned(),

        request_seq: req.seq,
        command: "initialize".to_owned(),

        success: true,
        message: None,

        body: Some(Capabilities::default())
    };

    let response_body = serde_json::to_vec(&init_resp).unwrap();

    let response_header = format!("Content-Length: {}\r\n\r\n", response_body.len());

    let mut stdout = io::stdout();

    debug!("> {}", response_header.trim_end());
    debug!("> {:?}", init_resp);
    stdout.write(&response_header.as_bytes())?;
    stdout.write(&response_body)?;

    let init_evt = InitializedEvent {
        seq: 2,
        type_: "event".to_owned(),

        event: "initialized".to_owned(),

        body: None,
    };

    let response_body = serde_json::to_vec(&init_evt).unwrap();

    let response_header = format!("Content-Length: {}\r\n\r\n", response_body.len());

    debug!("> {}", response_header.trim_end());
    debug!("> {:?}", init_evt);
    stdout.write(&response_header.as_bytes())?;
    stdout.write(&response_body)?;
    
    // look for other request

    loop {
        let mut header = String::new();

        io::stdin().read_line(&mut header)?;
        debug!("< {}", header.trim_end());

        // we should read an empty line here
        let mut buff = String::new();
        io::stdin().read_line(&mut buff)?;

        let len = get_content_len(&header);

        let mut content = vec![0u8; len];
        let bytes_read = io::stdin().read(&mut content)?;

        assert!(bytes_read == len);

        let req: Request = serde_json::from_slice(&content).unwrap();
        debug!("< {:?}", req);
    }
}

fn get_content_len(header: &str) -> usize {
    let mut parts = header.trim_end().split_ascii_whitespace();

    // discard first part
    parts.next().unwrap();

    parts.next().unwrap().parse::<usize>().unwrap()
}
