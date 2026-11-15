use agent_api_transport_verification::transport::{AgentServer, Dispatch};
use serde_json::json;
use std::io::{self, BufRead, Write};
use std::path::Path;
use std::sync::Arc;

fn main() -> io::Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    let mode = args.get(1).and_then(|arg| arg.to_str()).unwrap_or("");
    if mode == "watchdog-stall" {
        println!("watchdog-ready");
        io::stdout().flush()?;
        loop {
            std::thread::park();
        }
    }
    let path = Path::new(args.get(2).expect("owned fixture directory"));
    let dispatch: Dispatch = Arc::new(|_| json!({"success": true}));
    if mode == "try-host" {
        match AgentServer::start(path, dispatch) {
            Ok(server) => {
                println!("acquired");
                drop(server);
            }
            Err(_) => println!("blocked"),
        }
        return Ok(());
    }
    assert_eq!(mode, "host");
    let server = AgentServer::start(path, dispatch)?;
    println!("ready");
    io::stdout().flush()?;
    let mut command = String::new();
    io::stdin().lock().read_line(&mut command)?;
    assert_eq!(command.trim(), "stop");
    drop(server);
    println!("stopped");
    io::stdout().flush()
}
