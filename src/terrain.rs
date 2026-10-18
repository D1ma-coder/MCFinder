//! Optional, version-pinned vanilla JVM worker. Original game jars stay outside releases.
use crate::events::Control;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    process::{Child, ChildStdin, Command, Stdio},
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
    time::Duration,
};
type Reply = Result<Value, String>;
type Pending = Arc<Mutex<HashMap<u64, mpsc::Sender<Reply>>>>;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {
    schema: u32,
    version: String,
    java: PathBuf,
    classpath: Vec<PathBuf>,
    source_sha256: String,
    files: HashMap<PathBuf, String>,
}
pub struct Terrain {
    child: Mutex<Child>,
    input: Mutex<ChildStdin>,
    pending: Pending,
    next: AtomicU64,
}
impl Terrain {
    pub(crate) fn start(
        path: &Path,
        version: &str,
        cores: usize,
        control: &Control,
    ) -> Result<Arc<Self>, String> {
        let config: Config = serde_json::from_slice(
            &fs::read(path).map_err(|e| format!("Terrain runtime {}: {e}", path.display()))?,
        )
        .map_err(|e| e.to_string())?;
        if config.schema != 1 || config.version != version || version != "26.2" {
            return Err(
                "Exact terrain runtime requires the matching vanilla Java 26.2 profile".into(),
            );
        }
        let digest = format!(
            "{:x}",
            Sha256::digest(
                [
                    include_bytes!("../java/MCFinderTerrain.java").as_slice(),
                    include_bytes!("../java/mcfinder/terrain/MCFinderClimateCache.java").as_slice()
                ]
                .concat()
            )
        );
        if digest != config.source_sha256 {
            return Err("Terrain adapter changed; rerun scripts/prepare-terrain.py".into());
        }
        if config.files.is_empty() || config.classpath.is_empty() {
            return Err("Empty terrain runtime".into());
        }
        for (file, expected) in &config.files {
            let actual = format!(
                "{:x}",
                Sha256::digest(fs::read(file).map_err(|e| format!("{}: {e}", file.display()))?)
            );
            if actual != *expected {
                return Err(format!(
                    "Terrain runtime checksum mismatch: {}",
                    file.display()
                ));
            }
        }
        let classpath = std::env::join_paths(&config.classpath).map_err(|e| e.to_string())?;
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path.with_extension("log"))
            .map_err(|e| e.to_string())?;
        let mut child = Command::new(&config.java)
            .args(["-Xms64m", "-Xmx768m", "-cp"])
            .arg(classpath)
            .arg("MCFinderTerrain")
            .arg(cores.to_string())
            .current_dir(path.parent().unwrap_or_else(|| Path::new(".")))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(log)
            .spawn()
            .map_err(|e| format!("Start vanilla terrain worker: {e}"))?;
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (tx, rx) = mpsc::channel();
        pending.lock().unwrap().insert(0, tx);
        let replies = pending.clone();
        std::thread::spawn(move || {
            let mut error = "Vanilla terrain worker exited; see runtime.log".to_owned();
            for line in BufReader::new(output).lines() {
                let result = line.map_err(|e| e.to_string()).and_then(|l| {
                    serde_json::from_str::<Value>(&l).map_err(|e| format!("Terrain protocol: {e}"))
                });
                match result {
                    Ok(value) => {
                        if let Some(id) = value["id"].as_u64()
                            && let Some(tx) = replies.lock().unwrap().remove(&id)
                        {
                            let reply = if let Some(e) = value["error"].as_str() {
                                Err(e.to_owned())
                            } else {
                                Ok(value)
                            };
                            let _ = tx.send(reply);
                        }
                    }
                    Err(e) => {
                        error = e;
                        break;
                    }
                }
            }
            for (_, tx) in replies.lock().unwrap().drain() {
                let _ = tx.send(Err(error.clone()));
            }
        });
        let terrain = Arc::new(Self {
            child: Mutex::new(child),
            input: Mutex::new(input),
            pending,
            next: AtomicU64::new(1),
        });
        let started = std::time::Instant::now();
        loop {
            if !control.wait() {
                return Err("Search cancelled".into());
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(reply) => {
                    let ready = reply?;
                    if ready["ready"] != true || ready["version"] != version {
                        return Err("Invalid terrain worker handshake".into());
                    }
                    break;
                }
                Err(mpsc::RecvTimeoutError::Timeout)
                    if started.elapsed() < Duration::from_secs(60) => {}
                _ => return Err("Vanilla terrain worker startup failed; see runtime.log".into()),
            }
        }
        Ok(terrain)
    }
    pub(crate) fn query(&self, mut request: Value, control: &Control) -> Reply {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        request["id"] = json!(id);
        let (tx, rx) = mpsc::channel();
        self.pending.lock().unwrap().insert(id, tx);
        let result = (|| {
            let mut input = self.input.lock().unwrap();
            serde_json::to_writer(&mut *input, &request).map_err(|e| e.to_string())?;
            input
                .write_all(b"\n")
                .and_then(|_| input.flush())
                .map_err(|e| e.to_string())
        })();
        if let Err(e) = result {
            self.pending.lock().unwrap().remove(&id);
            return Err(e);
        }
        let deadline = std::time::Instant::now() + Duration::from_secs(120);
        loop {
            if std::time::Instant::now() >= deadline {
                self.pending.lock().unwrap().remove(&id);
                return Err("Vanilla terrain request timed out".into());
            }
            if !control.wait() {
                self.pending.lock().unwrap().remove(&id);
                return Err("Search cancelled".into());
            }
            match rx.recv_timeout(Duration::from_millis(50)) {
                Ok(reply) => return reply,
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(_) => return Err("Vanilla terrain worker disconnected".into()),
            }
        }
    }
}
impl Drop for Terrain {
    fn drop(&mut self) {
        if let Ok(child) = self.child.get_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
