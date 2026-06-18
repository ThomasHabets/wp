use std::io::{Read, Write};
use std::os::unix::process::ExitStatusExt;
use std::process::Command;
use std::process::Stdio;
use std::sync::mpsc;
use std::thread;

use clap::Parser;

const ESC: u8 = b'_';
const EESC: u8 = b'-';

const EOF: u8 = b'Z';
const SOB: u8 = b'<';
const EOB: u8 = b'>';

const ESOB: u8 = b'[';
const EEOB: u8 = b']';

#[derive(Debug)]
enum State {
    Idle,
    Data,
    DataEsc,
    Eof,
}

#[derive(Debug)]
struct Decapper {
    state: State,
    count: u64,
}

impl Decapper {
    fn new() -> Decapper {
        Decapper {
            state: State::Idle,
            count: 0,
        }
    }
    // Return:
    // * Some(true) if EOF found.
    // * Some(false) if we should carry on.
    // * None if we should stop. Bad input.
    fn add(&mut self, mut next: &std::process::ChildStdin, x: &[u8]) -> Option<bool> {
        let mut out = Vec::new();
        let mut ret = false;
        for ch in x {
            self.count += 1;
            match self.state {
                State::Idle => match *ch {
                    EOF => {
                        ret = true;
                        self.state = State::Eof;
                    }
                    SOB => {
                        self.state = State::Data;
                    }
                    other => {
                        let u = &[other];
                        let s = std::str::from_utf8(u).unwrap_or("<binary>");
                        eprintln!(
                            "wp: got invalid command character in input at index {}: {} ({})",
                            self.count - 1,
                            other,
                            s
                        );
                        return None;
                    }
                },
                State::Data => match *ch {
                    EOB => {
                        self.state = State::Idle;
                    }
                    ESC => {
                        self.state = State::DataEsc;
                    }
                    _ => {
                        out.push(*ch);
                    }
                },
                State::DataEsc => match *ch {
                    EESC => {
                        out.push(ESC);
                        self.state = State::Data;
                    }
                    ESOB => {
                        out.push(SOB);
                        self.state = State::Data;
                    }
                    EEOB => {
                        out.push(EOB);
                        self.state = State::Data;
                    }
                    other => {
                        let u = &[other];
                        let s = std::str::from_utf8(u).unwrap_or("<binary>");
                        eprintln!(
                            "wp: invalid escape input at index {}: {} ({})",
                            self.count - 1,
                            other,
                            s
                        );
                        return None;
                    }
                },
                State::Eof => {
                    let u = &[*ch];
                    let s = std::str::from_utf8(u).unwrap_or("<binary>");
                    eprintln!(
                        "wp: got input after EOF at index {}: {ch} ({s})",
                        self.count - 1,
                    );
                    return None;
                }
            };
        }
        if out.is_empty() {
            return Some(ret);
        }
        match next.write_all(out.as_slice()) {
            Ok(_) => Some(ret),
            Err(e) => {
                eprintln!("wp: Error writing to stdout: {}", e);
                None
            }
        }
    }
}

fn encap(inp: &[u8]) -> Vec<u8> {
    let mut out = vec![SOB];
    for ch in inp {
        match *ch {
            ESC => {
                out.push(ESC);
                out.push(EESC);
            }
            SOB => {
                out.push(ESC);
                out.push(ESOB);
            }
            EOB => {
                out.push(ESC);
                out.push(EEOB);
            }
            _ => {
                out.push(*ch);
            }
        }
    }
    out.push(EOB);
    out
}

#[derive(clap::Parser)]
#[clap(name = "wp", version)]
struct Opt {
    #[arg(short, long)]
    input: bool,

    #[arg(short, long)]
    output: bool,

    command: String,

    #[arg(trailing_var_arg = true,
        allow_hyphen_values = true,
        num_args = 1..,
    )]
    args: Vec<String>,
}

fn main() {
    let opt = Opt::parse();

    // TODO: move all but flag parsing to lib.
    let mut prep = Command::new(&opt.command);
    if opt.output {
        prep.stdout(Stdio::piped());
    }
    if opt.input {
        prep.stdin(Stdio::piped());
    }

    let mut child = prep
        .args(&opt.args[0..])
        .spawn()
        .expect("failed to execute child");

    let (ok_out_tx, ok_out_rx) = mpsc::channel();

    let othread = (|| {
        if opt.output {
            let mut childout = child
                .stdout
                .take()
                .expect("failed to take ownership of child stdout");
            return thread::spawn(move || {
                loop {
                    let mut buffer = vec![0; 4096_usize];
                    let n = match childout.read(&mut buffer) {
                        Ok(n) => n,
                        Err(e) => {
                            eprintln!("wp: failed to read from child stdout: {}", e);
                            return false;
                        }
                    };
                    if n == 0 {
                        break;
                    }
                    if let Err(e) = std::io::stdout().write_all(&encap(&buffer[0..n])) {
                        eprintln!("wp: Error writing to stdout: {e}");
                        return false;
                    }
                }
                match ok_out_rx.recv() {
                    Ok(send_eof) => {
                        if send_eof && let Err(e) = std::io::stdout().write_all(&[EOF]) {
                            eprintln!("wp: Error writing EOF to stdout: {e}");
                            false
                        } else {
                            true
                        }
                    }
                    Err(e) => {
                        eprintln!("wp: Did not get EOF success status. Assuming not: {e}");
                        false
                    }
                }
            });
        }
        thread::spawn(move || true)
    })();

    let (ctx, crx) = mpsc::channel();

    let ithread = (|| {
        if opt.input {
            let childin = child
                .stdin
                .take()
                .expect("failed to take ownership of child stdin");
            return thread::spawn(move || {
                let mut dec = Decapper::new();
                let mut saw_eof = false;
                loop {
                    let mut buffer = vec![0; 4096_usize];
                    let n = std::io::stdin()
                        .read(&mut buffer)
                        .expect("failed to read from stdin");
                    if n == 0 {
                        if saw_eof {
                            drop(childin);
                            ctx.send(child.wait())
                                .expect("failed to send wait status from ithread");
                            return true;
                        }
                        break;
                    }
                    let buf = &buffer[0..n];
                    match dec.add(&childin, buf) {
                        Some(true) => {
                            saw_eof = true;
                        }
                        Some(false) => (),
                        None => break,
                    }
                }
                if let Err(e) = child.kill() {
                    eprintln!("wp: failed to kill child: {}", e);
                }
                let ws = child.wait();
                if let Ok(ecode) = ws
                    && ecode.success()
                {
                    eprintln!("wp: Killed child, but it died a happy process");
                }
                ctx.send(ws)
                    .expect("failed to send wait status from ithread after kill");
                // TODO: Ideally we would send a fake error if
                // kill results in exit code 0, but I can't find
                // how to do that.
                //
                // Instead we're sending the success, but having
                // the thread return false.
                false
            });
        }
        thread::spawn(move || {
            ctx.send(child.wait())
                .expect("failed to send wait status from fake ithread");
            true
        })
    })();

    let ecode = crx
        .recv()
        .expect("main thread getting back client object")
        .expect("wait success");
    let child_exit_code = if ecode.success() {
        None
    } else if let Some(code) = ecode.code() {
        eprintln!("wp: Subprocess died with exit code {}", code);
        Some(code)
    } else if let Some(sig) = ecode.signal() {
        eprintln!("wp: died due to signal {}", sig);
        Some(1)
    } else {
        eprintln!("wp: with no exit code and no signal");
        Some(1)
    };

    let ithread_ok = ithread.join().expect("failed to join input reading");
    if opt.output {
        let _ = ok_out_tx.send(child_exit_code.is_none() && ithread_ok);
    }
    let othread_ok = othread.join().expect("failed to join output writing");
    if let Some(code) = child_exit_code {
        std::process::exit(code);
    }
    if !ithread_ok {
        std::process::exit(1);
    }
    if !othread_ok {
        std::process::exit(1);
    }
}
