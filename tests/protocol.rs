use serde_json::{Value, json};
use std::{
    io::{BufRead, BufReader, Write},
    process::{Command, Stdio},
    sync::mpsc,
    time::{Duration, Instant},
};
struct Server {
    child: std::process::Child,
    input: std::process::ChildStdin,
    events: mpsc::Receiver<(Instant, Value)>,
    last_seq: u64,
}
impl Server {
    fn new() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_mcfinder"))
            .arg("serve")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = child.stdout.take().unwrap();
        let (tx, events) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(output).lines() {
                let line = line.unwrap();
                let event: Value =
                    serde_json::from_str(&line).expect("stdout must contain only JSONL");
                if tx.send((Instant::now(), event)).is_err() {
                    break;
                }
            }
        });
        let mut s = Self {
            child,
            input,
            events,
            last_seq: 0,
        };
        assert_eq!(s.read().1["event"], "hello");
        s
    }
    fn send(&mut self, v: Value) {
        writeln!(self.input, "{v}").unwrap();
        self.input.flush().unwrap();
    }
    fn read(&mut self) -> (Instant, Value) {
        let (t, e) = self.events.recv_timeout(Duration::from_secs(8)).unwrap();
        let seq = e["seq"].as_u64().unwrap();
        assert!(seq > self.last_seq);
        self.last_seq = seq;
        assert_eq!(e["protocol"], 1);
        (t, e)
    }
    fn until(&mut self, event: &str) -> Value {
        loop {
            let (_, e) = self.read();
            if e["event"] == event {
                return e;
            }
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
#[test]
fn persistent_backend_streams_progress_accepts_controls_and_reuses_process() {
    let mut s = Server::new();
    s.send(json!({"cmd":"capabilities","id":"caps"}));
    assert_eq!(s.until("capabilities")["heartbeat_seconds"], 1);
    writeln!(s.input, "{{invalid}}").unwrap();
    s.input.flush().unwrap();
    assert_eq!(s.until("error")["id"], Value::Null);
    s.send(json!({"cmd":"start","id":"job1","settings":{"query":{"version":"1.18.2","radius":0,"rules":[{"type":"near","a":"mansion","b":"village","distance":0}]},"count":1000000000000_u64,"placement_only":true,"cores":1}}));
    assert_eq!(s.until("ready")["job_id"], "job1");
    let first = s.until("progress");
    assert!(
        first["state"]["bases_tested"]
            .as_str()
            .unwrap()
            .parse::<u64>()
            .unwrap()
            > 0
    );
    s.send(json!({"cmd":"pause","id":"p"}));
    assert_eq!(s.until("ack")["action"], "pause");
    assert_eq!(s.until("progress")["state"]["paused"], true);
    s.send(json!({"cmd":"resume","id":"r"}));
    assert_eq!(s.until("ack")["action"], "resume");
    s.send(json!({"cmd":"cancel","id":"c"}));
    let done = s.until("finished");
    assert_eq!(done["summary"]["status"], "cancelled");
    s.send(json!({"cmd":"start","id":"job2","settings":{"script":{"source":"fn configure(){#{version:\"1.18.2\",radius:0,rules:[]}} fn matches(w,c){#{seed:w.seed_bits()}}"},"count":2,"cores":1}}));
    let result = s.until("match");
    assert_eq!(result["job_id"], "job2");
    assert_eq!(result["hit"]["data"]["seed"], "0");
    assert_eq!(s.until("finished")["summary"]["status"], "complete");
    s.send(json!({"cmd":"shutdown","id":"bye"}));
    assert_eq!(s.until("ack")["action"], "shutdown");
    assert!(s.child.wait().unwrap().success());
}
#[test]
fn oneshot_backend_wraps_matches_and_finished_events() {
    let out = Command::new(env!("CARGO_BIN_EXE_mcfinder"))
        .args([
            "search",
            "--script",
            "examples/scripts/slime-cluster.rhai",
            "--count",
            "10",
            "--ui",
            "backend",
            "--cores",
            "1",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let events: Vec<Value> = String::from_utf8(out.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(events[0]["event"], "started");
    assert_eq!(events.last().unwrap()["event"], "finished");
    assert!(
        events
            .iter()
            .all(|e| e["protocol"] == 1 && e["job_id"] == "cli")
    );
}
