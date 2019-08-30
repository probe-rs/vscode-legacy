use std::fs::File;
use std::io;
use std::io::Read;

use std::env;

use debugserver_types::InitializeRequest;
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

    loop {
        let mut buff = String::new();
        io::stdin().read_line(&mut buff)?;

        debug!("< {}", buff.trim_end());
    }
}

fn get_content_len(header: &str) -> usize {
    let mut parts = header.trim_end().split_ascii_whitespace();

    // discard first part
    parts.next().unwrap();

    parts.next().unwrap().parse::<usize>().unwrap()
}
