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
        if s.next.len() < 3 {
            bail!("insufficient observed queue")
        }
        self.send(&Start {
            kind: "start",
            board: s.board.tbp_rows(),
            queue: s.queue(),
            hold: s.hold,
            combo: s.combo,
            back_to_back: s.back_to_back,
        })
    }
    pub fn suggest(&mut self) -> Result<Vec<Placement>> {
        let deadline = std::time::Instant::now() + self.timeout;
        loop {
            self.send(&json!({"type":"suggest"}))?;
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            let message = self
                .rx
                .recv_timeout(remaining)
                .context("TBP suggestion deadline/stdout disconnected")??;
            match message {
                Message::Suggestion { moves, .. } if !moves.is_empty() => return Ok(moves),
                Message::Suggestion { .. } => {}
                m => bail!("expected suggestion, got {m:?}"),
            }
            if std::time::Instant::now() >= deadline {
                bail!("engine produced no moves within search budget")
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    pub fn play(&mut self, p: Placement) -> Result<()> {
        self.send(&json!({"type":"play","move":p}))
    }
    pub fn new_piece(&mut self, p: Piece) -> Result<()> {
        self.send(&json!({"type":"new_piece","piece":p}))
    }
    pub fn stop(&mut self) -> Result<()> {
        self.send(&json!({"type":"stop"}))
    }
    pub fn resync(&mut self, s: &Snapshot) -> Result<()> {
        self.stop()?;
        self.start(s)
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
