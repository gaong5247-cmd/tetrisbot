//! Independent newline-delimited JSON child-process TBP client.
use anyhow::{Context, Result, bail};
use crossbeam_channel::{Receiver, bounded};
use game_state::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    time::Duration,
};
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Message {
    Info {
        name: String,
        version: String,
        author: String,
        features: Vec<String>,
    },
    Ready,
    Suggestion {
        moves: Vec<Placement>,
        #[serde(default)]
        move_info: Option<Value>,
    },
    Error {
        reason: String,
    },
}
#[derive(Serialize)]
struct Start {
    #[serde(rename = "type")]
    kind: &'static str,
    board: Vec<[Option<char>; 10]>,
    queue: Vec<Piece>,
    hold: Option<Piece>,
    combo: u32,
    back_to_back: bool,
}
pub struct Client {
    child: Child,
    stdin: Option<ChildStdin>,
    // Avoid stop() twice per placed piece when the worker already stopped
    // this synthetic current-piece search after verifying a hard drop.
    running: bool,
    rx: Receiver<Result<Message>>,
    pub info: String,
    pub timeout: Duration,
    reader: Option<std::thread::JoinHandle<()>>,
}
impl Client {
    pub fn spawn(path: impl AsRef<Path>, timeout: Duration) -> Result<Self> {
        let mut cmd = Command::new(path.as_ref());
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            cmd.creation_flags(0x08000000);
        }
        let mut child = cmd.spawn().context("launch Cold Clear 2 executable")?;
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().context("stdout pipe")?;
        let (tx, rx) = bounded(32);
        let reader = std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let msg = line
                    .map_err(anyhow::Error::from)
                    .and_then(|s| serde_json::from_str(&s).map_err(anyhow::Error::from));
                if tx.send_timeout(msg, Duration::from_millis(100)).is_err() {
                    return;
                }
            }
        });
        let mut c = Self {
            child,
            stdin,
            running: false,
            rx,
            info: String::new(),
            timeout,
            reader: Some(reader),
        };
        match c.receive()? {
            Message::Info { name, version, .. } => c.info = format!("{name} {version}"),
            m => bail!("expected info, got {m:?}"),
        }
        c.send(&json!({"type":"rules"}))?;
        if !matches!(c.receive()?, Message::Ready) {
            bail!("engine not ready")
        };
        Ok(c)
    }
    pub fn send(&mut self, v: &impl Serialize) -> Result<()> {
        if let Some(status) = self.child.try_wait()? {
            bail!("engine exited: {status}")
        }
        let out = self.stdin.as_mut().context("engine stdin closed")?;
        serde_json::to_writer(&mut *out, v)?;
        out.write_all(b"\n")?;
        out.flush()?;
        Ok(())
    }
    pub fn receive(&self) -> Result<Message> {
        self.rx
            .recv_timeout(self.timeout)
            .context("TBP timeout or stdout disconnected")?
    }
    pub fn start(&mut self, s: &Snapshot) -> Result<()> {
        self.start_with_mode(s, false)
    }
    /// Never speculate a 7-bag from invisible NEXT. Cold Clear 2's
    /// Randomizer::Unknown expands the single observed current-piece layer
    /// without an unbounded imaginary future search.
    pub fn start_with_mode(&mut self, s: &Snapshot, _restricted: bool) -> Result<()> {
        if s.queue().is_empty() {
            bail!("no observed current tetromino")
        }
        self.send(&Start {
            kind: "start",
            board: s.board.tbp_rows(),
            queue: s.queue(),
            hold: s.hold,
            combo: s.combo,
            back_to_back: s.back_to_back,
        })?;
        self.running=true;
        Ok(())
    }
    pub fn suggest(&mut self) -> Result<Vec<Placement>> {
        let deadline = std::time::Instant::now() + self.timeout;
        loop {
            self.send(&json!({"type":"suggest"}))?;
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let message = match self.rx.recv_timeout(remaining) {
                Ok(message) => message?,
                Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                    if let Some(status) = self.child.try_wait()? {
                        bail!("Cold Clear 2 child exited during suggestion: {status}")
                    }
                    bail!("Cold Clear 2 did not respond to TBP suggest within {}ms; process still alive",self.timeout.as_millis())
                }
                Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                    let status = self.child.try_wait()?;
                    bail!("Cold Clear 2 stdout disconnected while awaiting suggestion; process={status:?}")
                }
            };
            match message {
                Message::Suggestion { moves, .. } if !moves.is_empty() => return Ok(moves),
                Message::Suggestion { .. } => {}
                m => bail!("expected suggestion, got {m:?}"),
            }
            if std::time::Instant::now() >= deadline {
                bail!("engine produced no moves within search budget")
            }
            // Cold Clear 2 expands the single observed piece quickly. Large
            // polling gaps add avoidable thinking latency on every placement.
            std::thread::sleep(Duration::from_millis(8));
        }
    }
    pub fn play(&mut self, p: Placement) -> Result<()> {
        self.send(&json!({"type":"play","move":p}))
    }
    pub fn new_piece(&mut self, p: Piece) -> Result<()> {
        self.send(&json!({"type":"new_piece","piece":p}))
    }
    pub fn stop(&mut self) -> Result<()> {
        if !self.running { return Ok(()) }
        self.send(&json!({"type":"stop"}))?;
        self.running=false;
        Ok(())
    }
    pub fn resync(&mut self, s: &Snapshot) -> Result<()> {
        self.resync_with_mode(s, false)
    }
    pub fn resync_with_mode(&mut self, s: &Snapshot, restricted: bool) -> Result<()> {
        self.stop()?;
        self.start_with_mode(s, restricted)
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        let _ = self.send(&json!({"type":"quit"}));
        self.stdin.take();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(t) = self.reader.take() {
            let _ = t.join();
        }
    }
}
