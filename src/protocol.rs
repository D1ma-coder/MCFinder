//! Versioned JSONL protocol over stdin/stdout. One active search per process.
use crate::{
    backend,
    engine::{self, Checkpoint, ExecutionMode, RunOptions, Settings},
    events::{Control, Event, Observer, Summary},
    script::{self, ScriptExtension, ScriptSpec},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Read, Write},
    path::PathBuf,
    sync::{Arc, Mutex},
    thread::JoinHandle,
};

#[derive(Clone)]
pub struct JsonSink(Arc<Mutex<(Box<dyn Write + Send>, u64)>>);
impl JsonSink {
    pub fn stdout() -> Self {
        Self::new(Box::new(io::stdout()))
    }
    pub fn new(writer: Box<dyn Write + Send>) -> Self {
        Self(Arc::new(Mutex::new((writer, 0))))
    }
    pub fn send(&self, mut value: Value) -> Result<(), String> {
        let mut state = self.0.lock().map_err(|e| e.to_string())?;
        state.1 += 1;
        let object = value
            .as_object_mut()
            .ok_or("Protocol event must be an object")?;
        object.insert("protocol".into(), json!(1));
        object.insert("seq".into(), json!(state.1));
        serde_json::to_writer(&mut state.0, &value).map_err(|e| e.to_string())?;
        state.0.write_all(b"\n").map_err(|e| e.to_string())?;
        state.0.flush().map_err(|e| e.to_string())
    }
    pub fn observer(&self, job_id: String) -> Observer {
        let sink = self.clone();
        Arc::new(move |event| {
            let mut value = serde_json::to_value(event).map_err(|e| e.to_string())?;
            value["job_id"] = json!(job_id);
            sink.send(value)
        })
    }
}
#[derive(Deserialize)]
#[serde(tag = "cmd", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Capabilities {
        id: String,
    },
    Compile {
        id: String,
        script: ScriptSpec,
    },
    Start {
        id: String,
        settings: Box<Settings>,
        checkpoint: Option<PathBuf>,
        max_results: Option<u64>,
        #[serde(default)]
        append: bool,
    },
    Restore {
        id: String,
        checkpoint: PathBuf,
        cores: Option<usize>,
        mode: Option<ExecutionMode>,
        gpu_index: Option<usize>,
        max_results: Option<u64>,
    },
    Status {
        id: String,
    },
    Pause {
        id: String,
    },
    Resume {
        id: String,
    },
    Cancel {
        id: String,
    },
    Shutdown {
        id: String,
    },
}
struct Job {
    id: String,
    control: Control,
    thread: Option<JoinHandle<()>>,
}
impl Job {
    fn is_running(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| !t.is_finished())
    }
}
impl Drop for Job {
    fn drop(&mut self) {
        self.control.cancel();
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}
fn spawn(id: String, settings: Settings, options: RunOptions, sink: &JsonSink) -> Job {
    let control = Control::new();
    let worker = control.clone();
    let observer = sink.observer(id.clone());
    let thread = std::thread::spawn(move || {
        let panic_observer = observer.clone();
        if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            engine::run(settings, options, worker.clone(), observer)
        }))
        .is_err()
        {
            worker.cancel();
            let _ = panic_observer(Event::Error {
                message: "Search worker panicked".into(),
            });
            let _ = panic_observer(Event::Finished {
                summary: Summary {
                    status: "failed".into(),
                    state: worker.snapshot(),
                    error: Some("Search worker panicked".into()),
                },
            });
        }
    });
    Job {
        id,
        control,
        thread: Some(thread),
    }
}
pub fn capabilities() -> Value {
    json!({"event":"capabilities", "engine_version":env!("CARGO_PKG_VERSION"), "protocol_version":1,
        "interfaces":["tui","backend","rust-sdk"], "modes":["cpu-only","cpu-and-gpu"],
        "scripts":{"language":"rhai","functions":["configure","matches"],"max_source_bytes":1048576,"default_max_operations":100000,
            "world_api":["seed","seed_bits","lower48","upper16","position","structures","viable","biome","spawn","slime_chunk"],
            "gpu":"native declarative filters only; custom predicates execute on CPU"},
        "versions":backend::VERSIONS, "experimental_versions_from":"1.21.5",
        "world_data":{"structures":true,"biomes":true,"spawn":"approximate","blocks":false,"terrain_height":false,"loot":false},
        "heartbeat_seconds":1, "counter_encoding":"decimal strings", "max_active_jobs":1})
}
pub fn serve() -> engine::Result<()> {
    serve_with_extensions(Vec::new())
}
pub fn serve_with_extensions(extensions: Vec<Arc<dyn ScriptExtension>>) -> engine::Result<()> {
    let sink = JsonSink::stdout();
    let active: Arc<Mutex<Option<Control>>> = Arc::new(Mutex::new(None));
    let interrupt = active.clone();
    ctrlc::set_handler(move || {
        if let Some(c) = &*interrupt.lock().unwrap() {
            c.cancel();
        }
    })?;
    serve_reader_with_extensions(io::stdin().lock(), sink, active, extensions)
}
pub fn serve_reader<R: BufRead>(
    reader: R,
    sink: JsonSink,
    active: Arc<Mutex<Option<Control>>>,
) -> engine::Result<()> {
    serve_reader_with_extensions(reader, sink, active, Vec::new())
}
pub fn serve_reader_with_extensions<R: BufRead>(
    mut reader: R,
    sink: JsonSink,
    active: Arc<Mutex<Option<Control>>>,
    extensions: Vec<Arc<dyn ScriptExtension>>,
) -> engine::Result<()> {
    sink.send(
        json!({"event":"hello","engine_version":env!("CARGO_PKG_VERSION"),"protocol_version":1}),
    )?;
    let mut job: Option<Job> = None;
    let mut buffer = Vec::new();
    loop {
        buffer.clear();
        // Keep inbound requests bounded, including a malicious line without '\n'.
        let n = (&mut reader)
            .take(2_097_153)
            .read_until(b'\n', &mut buffer)?;
        if n == 0 {
            break;
        }
        if buffer.len() > 2_097_152 {
            sink.send(json!({"event":"error","id":null,"message":"Request exceeds 2 MiB"}))?;
            if buffer.last() != Some(&b'\n') {
                return Err("Oversized unterminated request".into());
            }
            continue;
        }
        let parsed: Value = match serde_json::from_slice(&buffer) {
            Ok(v) => v,
            Err(e) => {
                sink.send(
                    json!({"event":"error","id":null,"message":format!("Invalid JSON: {e}")}),
                )?;
                continue;
            }
        };
        let id = parsed.get("id").and_then(Value::as_str).map(str::to_owned);
        if id.as_ref().is_none_or(|id| id.is_empty() || id.len() > 256) {
            sink.send(json!({"event":"error","id":id,"message":"id must be a non-empty string of at most 256 bytes"}))?;
            continue;
        }
        let command: Command = match serde_json::from_value(parsed) {
            Ok(v) => v,
            Err(e) => {
                sink.send(json!({"event":"error","id":id,"message":e.to_string()}))?;
                continue;
            }
        };
        let result: engine::Result<bool> = (|| {
            match command {
                Command::Capabilities { id } => {
                    let mut v = capabilities();
                    v["id"] = json!(id);
                    v["extensions"] =
                        json!(extensions.iter().map(|e| e.name()).collect::<Vec<_>>());
                    sink.send(v)?;
                }
                Command::Compile { id, script } => {
                    let query = script::Program::compile_with_extensions(
                        &script,
                        Control::new(),
                        &extensions,
                    )?
                    .query;
                    sink.send(json!({"event":"compiled","id":id,"query":query,"script_hash":script.hash()}))?;
                }
                Command::Start {
                    id,
                    settings,
                    checkpoint,
                    max_results,
                    append,
                } => {
                    if job.as_ref().is_some_and(|j| j.is_running()) {
                        return Err("A search is already running; cancel it first".into());
                    }
                    if let Some(j) = job.take() {
                        drop(j);
                    }
                    if max_results == Some(0) {
                        return Err("max_results must be positive or omitted".into());
                    }
                    if checkpoint.as_ref().is_some_and(|p| p.exists()) {
                        return Err("Checkpoint exists; use restore".into());
                    }
                    let new = spawn(
                        id,
                        *settings,
                        RunOptions {
                            checkpoint,
                            extensions: extensions.clone(),
                            max_results: max_results.unwrap_or(0),
                            append,
                            ..Default::default()
                        },
                        &sink,
                    );
                    *active.lock().unwrap() = Some(new.control.clone());
                    job = Some(new);
                }
                Command::Restore {
                    id,
                    checkpoint,
                    cores,
                    mode,
                    gpu_index,
                    max_results,
                } => {
                    if job.as_ref().is_some_and(|j| j.is_running()) {
                        return Err("A search is already running".into());
                    }
                    let mut saved: Checkpoint =
                        serde_json::from_reader(std::fs::File::open(&checkpoint)?)?;
                    if saved.schema != 3 || saved.engine != engine::ENGINE {
                        return Err("Checkpoint engine/schema mismatch".into());
                    }
                    if let Some(c) = cores {
                        saved.settings.cores = Some(c);
                    }
                    if let Some(m) = mode {
                        saved.settings.mode = m;
                        if matches!(m, ExecutionMode::CpuOnly) {
                            saved.settings.gpu_index = None;
                        }
                    }
                    if gpu_index.is_some() {
                        saved.settings.gpu_index = gpu_index;
                    }
                    if max_results == Some(0) {
                        return Err("max_results must be positive or omitted".into());
                    }
                    if let Some(j) = job.take() {
                        drop(j);
                    }
                    let new = spawn(
                        id,
                        saved.settings,
                        RunOptions {
                            next: Some(saved.next),
                            checkpoint: Some(checkpoint),
                            max_results: max_results.unwrap_or(0),
                            append: true,
                            resuming: true,
                            restored_plugins: Some(saved.plugins),
                            extensions: extensions.clone(),
                            ..Default::default()
                        },
                        &sink,
                    );
                    *active.lock().unwrap() = Some(new.control.clone());
                    job = Some(new);
                }
                Command::Status { id } => {
                    let j = job.as_ref().ok_or("No search has been started")?;
                    sink.send(json!({"event":"status","id":id,"job_id":j.id,"running":j.is_running(),"state":j.control.snapshot()}))?;
                }
                Command::Pause { id } | Command::Resume { id } | Command::Cancel { id } => {
                    let j = job
                        .as_ref()
                        .filter(|j| j.is_running())
                        .ok_or("No running search")?;
                    let cmd = String::from_utf8_lossy(&buffer);
                    let kind: Value = serde_json::from_str(&cmd)?;
                    let action = kind["cmd"].as_str().unwrap();
                    match action {
                        "pause" => j.control.pause(),
                        "resume" => j.control.resume(),
                        _ => j.control.cancel(),
                    }
                    sink.send(json!({"event":"ack","id":id,"job_id":j.id,"action":action}))?;
                }
                Command::Shutdown { id } => {
                    sink.send(json!({"event":"ack","id":id,"action":"shutdown"}))?;
                    return Ok(true);
                }
            }
            Ok(false)
        })();
        match result {
            Ok(true) => break,
            Ok(false) => {}
            Err(e) => sink.send(json!({"event":"error","id":id,"message":e.to_string()}))?,
        }
    }
    if let Some(job) = job {
        drop(job);
    }
    *active.lock().unwrap() = None;
    Ok(())
}
