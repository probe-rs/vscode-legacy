use assert_cmd::prelude::*;
use std::process::{Command, Stdio};

use std::io::{Read, Write};

use std::io::BufRead;
use std::io::BufReader;

use insta::assert_snapshot;

use std::str;

#[test]
fn basic_init_flow() -> Result<(), Box<dyn std::error::Error>> {
    let mut cmd = Command::cargo_bin("probe-rs-debugadapter")?;

    let initialize_request = format!(
        "Content-Length: 315\r\n\r\n{}",
        r#"{"command":"initialize","arguments":{"clientID":"vscode","clientName":"Visual Studio Code","adapterID":"probe_rs","pathFormat":"path","linesStartAt1":true,"columnsStartAt1":true,"supportsVariableType":true,"supportsVariablePaging":true,"supportsRunInTerminalRequest":true,"locale":"en-us"},"type":"request","seq":1}"#
    );

    let mut spawned = cmd.stdin(Stdio::piped()).stdout(Stdio::piped()).spawn()?;

    println!("Spawned...");

    {
        let mut cmd_in = spawned.stdin.take().unwrap();
        let cmd_out = spawned.stdout.take().unwrap();

        println!("Writing input..");

        cmd_in.write(initialize_request.as_bytes())?;

        let mut reader = BufReader::new(cmd_out);

        let mut header = String::new();

        println!("Waiting for a response..");

        reader.read_line(&mut header)?;

        println!("Got a header...");

        assert_snapshot!(header);

        let mut empty = String::new();

        assert_eq!(2, reader.read_line(&mut empty)?);

        let content_len = get_content_len(&header);

        let mut resp_body = vec![0u8; content_len];

        reader.read_exact(&mut resp_body)?;

        let response = str::from_utf8(&resp_body).unwrap();

        assert_snapshot!(response);
    }

    spawned.kill()?;

    Ok(())
}

fn get_content_len(header: &str) -> usize {
    let mut parts = header.trim_end().split_ascii_whitespace();

    // discard first part
    parts.next().unwrap();

    parts.next().unwrap().parse::<usize>().unwrap()
}
