use crate::{Error, Result, args::Args, config::Config, protocol};
use serde_json::Value;
use std::{io, process::Stdio, time::Duration};
use tempfile::TempDir;
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout},
    time::{Instant, timeout},
};

pub struct Client {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<ChildStdout>,
    temporary: Option<TempDir>,
    pub sequence: u64,
    timeout: Duration,
    desktop: bool,
    writing: bool,
}

impl Client {
    pub fn spawn(args: &Args, config: &Config) -> Result<Self> {
        let (mut command, temporary) = config.command(args)?;
        #[cfg(windows)]
        command.creation_flags(0x08000000); // CREATE_NO_WINDOW
        let mut child = command
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .kill_on_drop(true)
            .spawn()?;
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Ok(Self {
            child,
            stdin,
            stdout,
            temporary,
            sequence: 0,
            timeout: args.timeout,
            desktop: args.desktop,
            writing: false,
        })
    }

    async fn receive(&mut self, seq: Option<u64>, start: Instant) -> Result<Value> {
        let mut line = Vec::new();
        loop {
            line.clear();
            // The desktop broker caps its response payload at 4 MiB. Leave
            // room for the line ending. Engine transcripts have no wire cap.
            let limit = if self.desktop {
                4 * 1024 * 1024 + 2
            } else {
                u64::MAX
            };
            let n = (&mut self.stdout)
                .take(limit.saturating_add(1))
                .read_until(b'\n', &mut line)
                .await?;
            if n as u64 > limit {
                return Err(Error::runtime(
                    "Desktop response exceeds the protocol limit",
                ));
            }
            if n == 0 {
                return Err(Error::runtime(
                    "Engine stdout closed before a matching response",
                ));
            }
            let result = protocol::response(&line, seq);
            if start.elapsed() >= self.timeout {
                return Err(Error::runtime(
                    "Engine response exceeded the response deadline",
                ));
            }
            if let Some(value) = result? {
                return Ok(value);
            }
            // An always-readable pipe must still yield to the timer and Ctrl+C.
            tokio::task::yield_now().await;
        }
    }
    pub async fn ready(&mut self) -> Result<()> {
        let reply = timeout(self.timeout, self.receive(None, Instant::now()))
            .await
            .map_err(|_| Error::runtime("Engine startup exceeded the response deadline"))??;
        if reply["success"] != true {
            return Err(Error::runtime(format!("Engine startup failed: {reply}")));
        }
        Ok(())
    }

    pub async fn request(&mut self, bytes: &[u8]) -> Result<Value> {
        self.sequence += 1;
        let start = Instant::now();
        timeout(self.timeout, async {
            self.writing = true;
            self.stdin.write_all(bytes).await?;
            self.stdin.flush().await?;
            self.writing = false;
            self.receive(Some(self.sequence), start).await
        })
        .await
        .map_err(|_| Error::runtime("Engine request exceeded the response deadline"))?
    }

    pub async fn close(mut self) -> Result<()> {
        let grace = Duration::from_millis(250);
        let end = Instant::now() + Duration::from_secs(2);
        if self.child.try_wait()?.is_none() {
            // On a cancelled partial write, another frame would corrupt stdin.
            if !self.writing {
                let exit = format!(
                    "{{\"action\":\"exit\",\"request_id\":{}}}\n",
                    self.sequence + 1
                );
                let _ = timeout(grace, self.stdin.write_all(exit.as_bytes())).await;
            }
            drop(self.stdin);
            match timeout(grace, self.child.wait()).await {
                Ok(status) => {
                    status?;
                }
                Err(_) => {
                    self.child.start_kill()?;
                    tokio::time::timeout_at(end, self.child.wait())
                        .await
                        .map_err(|_| Error::runtime("Engine did not exit after termination"))??;
                }
            }
        }
        drop(self.stdout);
        if let Some(temp) = self.temporary {
            temp.close()?;
        }
        Ok(())
    }
}

pub fn reply(value: &Value) -> Result<()> {
    use std::io::Write;
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, value).map_err(io::Error::other)?;
    out.write_all(b"\n")?;
    out.flush()?;
    Ok(())
}
