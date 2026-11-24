mod args;
mod client;
mod config;
mod protocol;
mod standalone;

use args::Args;
use client::{Client, reply};
use config::Config;
use serde_json::{Value, json};
use std::{io, process::ExitCode, time::Duration};
use tokio::io::{AsyncBufReadExt, BufReader};

type Result<T> = std::result::Result<T, Error>;
struct Error {
    code: u8,
    message: String,
}
impl Error {
    fn input(message: impl Into<String>) -> Self {
        Self {
            code: 2,
            message: message.into(),
        }
    }
    fn runtime(message: impl Into<String>) -> Self {
        Self {
            code: 1,
            message: message.into(),
        }
    }
    fn interrupted() -> Self {
        Self {
            code: 130,
            message: "Interrupted".into(),
        }
    }
}
impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Self::runtime(e.to_string())
    }
}

async fn dispatch(
    args: &Args,
    config: &Config,
    client: &mut Option<Client>,
    wire: &[u8],
) -> Result<Value> {
    if client.is_none() {
        let owned = client.insert(Client::spawn(args, config)?);
        owned.ready().await?;
    }
    client.as_mut().unwrap().request(wire).await
}

async fn session(
    args: &Args,
    config: &Config,
    client: &mut Option<Client>,
    pending: &mut Option<Value>,
) -> Result<Option<Value>> {
    let mut input = BufReader::new(tokio::io::stdin());
    let mut line = Vec::new();
    loop {
        line.clear();
        if input.read_until(b'\n', &mut line).await? == 0 {
            return Ok(None);
        }
        if line.iter().all(u8::is_ascii_whitespace) {
            continue;
        }
        *pending = protocol::caller_id(&line);
        let record = (|| {
            let message: Value =
                serde_json::from_slice(&line).map_err(|e| Error::input(e.to_string()))?;
            if pending.is_none() {
                return Err(Error::input(
                    "Each request must be an object with an integer request_id",
                ));
            }
            let sequence = client.as_ref().map_or(1, |c| c.sequence + 1);
            let bytes = protocol::request(&message, args.desktop, sequence)?;
            Ok((bytes, message["action"] == "exit"))
        })();
        match record {
            Err(error) => {
                reply(&json!({"success":false,"request_id":pending,"error":error.message}))?
            }
            Ok((_, true)) => return Ok(Some(json!({"success":true,"request_id":pending}))),
            Ok((bytes, false)) => {
                let mut response = dispatch(args, config, client, &bytes).await?;
                response["request_id"] = pending.take().unwrap();
                reply(&response)?;
            }
        }
        *pending = None;
    }
}

async fn interrupt() {
    if tokio::signal::ctrl_c().await.is_err() {
        // Batch jobs without a console have no Ctrl+C source.
        std::future::pending::<()>().await;
    }
}

async fn run() -> Result<u8> {
    let Some(args) = args::parse()? else {
        println!("{}", args::HELP);
        return Ok(0);
    };
    let config = Config::load(&args)?;
    if args.request["action"] == "doctor" {
        reply(&config.doctor(&args)?)?;
        return Ok(0);
    }
    let is_session = args.request["action"] == "session";
    let mut client = None;
    let mut pending = None;
    let result = tokio::select! {
        biased;
        () = interrupt() => Err(Error::interrupted()),
        result = async {
            if is_session {session(&args,&config,&mut client,&mut pending).await}
            else {
                let wire = protocol::request(&args.request,args.desktop,1)?;
                dispatch(&args,&config,&mut client,&wire).await.map(Some)
            }
        } => result
    };
    let closed = match client {
        Some(c) => c.close().await,
        None => Ok(()),
    };
    match result.and_then(|v| closed.map(|_| v)) {
        Ok(Some(response)) => {
            reply(&response)?;
            Ok(u8::from(response["success"] != true))
        }
        Ok(None) => Ok(0),
        Err(error) if is_session => {
            if pending.is_some() {
                reply(&json!({"success":false,"request_id":pending,"error":error.message}))?;
            } else {
                eprintln!("{}", error.message);
            }
            Ok(if error.code == 130 { 130 } else { 1 })
        }
        Err(error) => Err(error),
    }
}

fn main() -> ExitCode {
    let result = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(Error::from)
        .and_then(|runtime| {
            let result = runtime.block_on(run());
            // A cancelled console read can remain blocked in the OS until another
            // keypress. All protocol children have already been closed and reaped.
            runtime.shutdown_timeout(Duration::from_millis(100));
            result
        });
    match result {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            if let Err(output) = reply(&json!({"success":false,"error":error.message})) {
                eprintln!("{}", output.message);
            }
            ExitCode::from(error.code)
        }
    }
}
