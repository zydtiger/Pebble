//! A task owns one stdio app-server child. No provider credentials or desktop RPCs.
use super::{CodexSettings, failure};
use crate::Error;
use serde_json::{Value, json};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Read, Write};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::{Duration, Instant};

const MESSAGE_CAP: u64 = 2 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum RpcError {
    Transport(Error),
    Rejected { code: Option<i64>, reason: String },
}
impl From<Error> for RpcError {
    fn from(error: Error) -> Self {
        Self::Transport(error)
    }
}
impl From<RpcError> for Error {
    fn from(error: RpcError) -> Self {
        match error {
            RpcError::Transport(error) => error,
            RpcError::Rejected { reason, .. } => failure(&reason),
        }
    }
}

pub(crate) struct Adapter {
    child: Child,
    stdin: ChildStdin,
    events: Receiver<Result<Value, ()>>,
    pending: VecDeque<Value>,
    next: u64,
}
impl Adapter {
    pub(crate) fn start(settings: &CodexSettings) -> Result<Self, Error> {
        let mut child = Command::new(super::settings::executable(&settings.executable)?)
            .args(["app-server", "--listen", "stdio://"])
            .current_dir(settings.cwd()?)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|_| {
                failure(
                    "Could not launch Codex. Select the installed executable in Codex settings.",
                )
            })?;
        let stdin = child.stdin.take().expect("piped input");
        let stdout = child.stdout.take().expect("piped output");
        let (tx, events) = mpsc::sync_channel(256);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(stdout);
            loop {
                let mut bytes = Vec::new();
                let read = Read::by_ref(&mut reader)
                    .take(MESSAGE_CAP + 1)
                    .read_until(b'\n', &mut bytes);
                if !matches!(read,Ok(n) if n>0 && n as u64<=MESSAGE_CAP) {
                    let _ = tx.send(Err(()));
                    break;
                }
                let value = serde_json::from_slice(&bytes).map_err(|_| ());
                let bad = value.is_err();
                if tx.send(value).is_err() || bad {
                    break;
                }
            }
        });
        let mut adapter = Self {
            child,
            stdin,
            events,
            pending: VecDeque::new(),
            next: 0,
        };
        adapter.rpc("initialize",json!({"clientInfo":{"name":"pebble","version":env!("CARGO_PKG_VERSION")},"capabilities":{"experimentalApi":true}}))?;
        adapter.send(json!({"method":"initialized"}))?;
        Ok(adapter)
    }
    pub(crate) fn send(&mut self, value: Value) -> Result<(), Error> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        self.stdin.write_all(&bytes).and_then(|_|self.stdin.flush()).map_err(|_|failure("Codex disconnected; execution may have occurred. Reconcile this task before continuing."))
    }
    pub(crate) fn rpc(&mut self, method: &str, params: Value) -> Result<Value, RpcError> {
        self.next += 1;
        let id = self.next;
        self.send(json!({"id":id,"method":method,"params":params}))?;
        let until = Instant::now() + Duration::from_secs(30);
        loop {
            let value=self.events.recv_timeout(until.saturating_duration_since(Instant::now()))
                .map_err(|_|failure("Codex request timed out or disconnected. Its outcome is uncertain; it will not be resent."))?
                .map_err(|_|failure("Codex closed or sent an unsupported protocol frame."))?;
            if value["id"] == id && value.get("method").is_none() {
                if value.get("error").is_some() {
                    let error = &value["error"];
                    let message = error["message"].as_str().unwrap_or("No reason supplied.");
                    return Err(RpcError::Rejected {
                        code: error["code"].as_i64(),
                        reason: format!(
                            "Codex rejected {method}: {message} Check the task input and local Codex configuration."
                        ),
                    });
                }
                return Ok(value["result"].clone());
            }
            if self.pending.len() >= 256 {
                return Err(failure("Codex event queue exceeded its bound.").into());
            }
            self.pending.push_back(value);
        }
    }
    pub(crate) fn event(&mut self) -> Result<Option<Value>, Error> {
        if let Some(value) = self.pending.pop_front() {
            return Ok(Some(value));
        }
        match self.events.recv_timeout(Duration::from_millis(100)) {
            Ok(Ok(value)) => Ok(Some(value)),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            _ => Err(failure(
                "Codex disconnected. The task was not resubmitted; reconcile its saved thread.",
            )),
        }
    }
}
impl Drop for Adapter {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

#[derive(Debug, Clone)]
pub struct CodexCapabilities {
    pub authenticated: bool,
    pub models: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub detail: String,
}
pub(crate) fn inspect(settings: &CodexSettings) -> Result<CodexCapabilities, Error> {
    let mut adapter = Adapter::start(settings)?;
    let account = adapter.rpc("account/read", json!({"refreshToken":false}))?;
    let models = adapter.rpc("model/list", json!({"limit":100}))?;
    let skills = adapter.rpc("skills/list", json!({"cwds":[settings.cwd()?]}))?;
    let mcp = adapter.rpc("mcpServerStatus/list", json!({"limit":100}))?;
    let names = |value: &Value, field: &str| {
        value["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v[field].as_str().map(str::to_owned))
            .collect()
    };
    Ok(CodexCapabilities {
        authenticated: !account["account"].is_null() || account["requiresOpenaiAuth"]==false,
        models:names(&models,"model"),
        skills:skills["data"].as_array().into_iter().flatten().flat_map(|v|v["skills"].as_array().into_iter().flatten()).filter(|v|v["enabled"]!=false).filter_map(|v|v["name"].as_str().map(str::to_owned)).collect(),
        mcp_servers:names(&mcp,"name"),
        detail:"Capabilities come from this local app-server and workspace. Desktop tools are not automatically available. Host integrations such as Calendar require permission in the actual task process; registered tools are not proof of access. Model and effort inherit local Codex configuration unless overridden.".into(),
    })
}
