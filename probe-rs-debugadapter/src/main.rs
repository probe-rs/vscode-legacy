use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;

use std::env;

use debugserver_types::InitializeRequest;
use serde_json;

struct Logger {
    file: Option<File>,
}

impl Logger {
    fn from_env_var(var_name: &str) -> io::Result<Logger> {
        let file = match env::var(var_name) {
            Ok(path) => Some(File::create(path)?),
            Err(_) => None,
        };

        Ok(Logger { file })
    }
}

impl Write for Logger {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self.file {
            Some(ref mut f) => f.write(buf),
            None => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self.file {
            Some(ref mut f) => f.flush(),
            None => Ok(()),
        }
    }
}

fn main() -> std::io::Result<()> {
    let mut logger = Logger::from_env_var("PROBE_RS_LOGFILE")?;
    let current_dir = env::current_dir()?;
    let args: Vec<String> = env::args().collect();

    writeln!(
        logger,
        "Debugger started in directory {}",
        current_dir.display()
    )?;

    writeln!(logger, "Arguments: {}", args.join(" "))?;

    // Simulate staying open, to see if something interesting happens

    let mut header = String::new();

    io::stdin().read_line(&mut header)?;
    writeln!(logger, "< {}", header.trim_end())?;

    // we should read an empty line here
    let mut buff = String::new();
    io::stdin().read_line(&mut buff)?;

    let len = get_content_len(&header);

    let mut content = vec![0u8; len];
    let bytes_read = io::stdin().read(&mut content)?;

    assert!(bytes_read == len);

    let req: InitializeRequest = serde_json::from_slice(&content).unwrap();
    writeln!(logger, "< {:?}", req)?;

    loop {
        let mut buff = String::new();
        io::stdin().read_line(&mut buff)?;
        writeln!(logger, "< {}", buff.trim_end())?;
    }
}

fn get_content_len(header: &str) -> usize {
    let mut parts = header.trim_end().split_ascii_whitespace();

    // discard first part
    parts.next().unwrap();

    parts.next().unwrap().parse::<usize>().unwrap()
}
