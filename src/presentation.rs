use crate::{
    engine::{self, RunOptions, Settings},
    events::{Control, Event, Observer, Snapshot, Summary},
    plan::Hit,
    protocol::JsonSink,
};
use clap::ValueEnum;
use crossterm::{
    event::{self, Event as TerminalEvent, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, Paragraph, Row, Sparkline, Table},
};
use std::{
    collections::VecDeque,
    io::{self, IsTerminal, Write},
    sync::{
        Arc,
        mpsc::{self, Receiver},
    },
    time::Duration,
};

#[derive(Debug, Clone, Copy, ValueEnum, Default)]
pub enum Interface {
    #[default]
    Auto,
    Tui,
    Backend,
    Plain,
}
fn plain() -> Observer {
    Arc::new(|event| {
        match event {
            Event::Match { hit } => {
                let stdout = io::stdout();
                let mut out = stdout.lock();
                serde_json::to_writer(&mut out, &hit).map_err(|e| e.to_string())?;
                out.write_all(b"\n")
                    .and_then(|_| out.flush())
                    .map_err(|e| e.to_string())?;
            }
            Event::Ready {
                query,
                cores,
                cpu_profile,
                gpu,
                ..
            } => {
                eprintln!(
                    "Java {} | {cores} CPU workers | {cpu_profile} | {}",
                    query.version,
                    gpu.unwrap_or_else(|| "cpu-only".into())
                );
            }
            Event::Progress { state } => eprintln!(
                "bases={} seeds={} found={} {:.0} bases/s {:.0} seeds/s {:.0} chunks/s phase={} cursor={}",
                state.bases_tested,
                state.seeds_evaluated,
                state.results_emitted,
                state.bases_per_second,
                state.seeds_per_second,
                state.chunks_per_second,
                state.phase,
                state.cursor
            ),
            Event::Finished { summary } => eprintln!(
                "{} bases={} checked={} hits={} elapsed={:.3}s rate={:.0} bases/s",
                summary.status,
                summary.state.bases_tested,
                summary.state.seeds_evaluated,
                summary.state.results_emitted,
                summary.state.elapsed_seconds,
                summary.state.average_bases_per_second
            ),
            _ => {}
        }
        Ok(())
    })
}
pub fn run(
    settings: Settings,
    options: RunOptions,
    interface: Interface,
) -> engine::Result<Summary> {
    let interface = match interface {
        Interface::Auto
            if io::stdout().is_terminal()
                && io::stderr().is_terminal()
                && io::stdin().is_terminal() =>
        {
            Interface::Tui
        }
        Interface::Auto => Interface::Plain,
        other => other,
    };
    let control = Control::new();
    let interrupt = control.clone();
    ctrlc::set_handler(move || interrupt.cancel())?;
    match interface {
        Interface::Tui => tui(settings, options, control),
        Interface::Backend => engine::run(
            settings,
            options,
            control,
            JsonSink::stdout().observer("cli".into()),
        ),
        _ => engine::run(settings, options, control, plain()),
    }
}
struct TerminalGuard;
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = disable_raw_mode();
        let _ = execute!(io::stderr(), LeaveAlternateScreen, crossterm::cursor::Show);
    }
}
#[derive(Default)]
struct Screen {
    state: Snapshot,
    hits: VecDeque<Hit>,
    rates: VecDeque<u64>,
    version: String,
    hardware: String,
    status: String,
    error: Option<String>,
    scroll: usize,
}
impl Screen {
    fn apply(&mut self, event: Event) {
        match event {
            Event::Started {
                engine_version,
                mode,
            } => {
                self.status = format!("Starting MCFinder {engine_version}");
                self.hardware = mode;
            }
            Event::Ready {
                query,
                cores,
                cpu_profile,
                gpu,
                ..
            } => {
                self.version = query.version;
                self.hardware = format!(
                    "{cores} CPU · {cpu_profile} · {}",
                    gpu.unwrap_or_else(|| "cpu-only".into())
                );
                self.status = "Searching".into();
            }
            Event::Progress { state } => {
                self.rates
                    .push_back(state.chunks_per_second.max(0.0) as u64);
                if self.rates.len() > 80 {
                    self.rates.pop_front();
                }
                self.state = state;
            }
            Event::Match { hit } => {
                self.hits.push_front(hit);
                if self.hits.len() > 200 {
                    self.hits.pop_back();
                }
            }
            Event::Error { message } => self.error = Some(message),
            Event::Finished { summary } => {
                self.rates
                    .push_back(summary.state.chunks_per_second.max(0.0) as u64);
                if self.rates.len() > 80 {
                    self.rates.pop_front();
                }
                self.status = summary.status;
                self.state = summary.state;
                self.error = summary.error;
            }
        }
    }
}
fn compact(value: f64) -> String {
    let value = value.max(0.0);
    let units = ["", "K", "M", "B", "T", "Q"];
    let mut scaled = value;
    let mut unit = 0;
    while scaled >= 999.5 && unit < units.len() - 1 {
        scaled /= 1000.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{scaled:.0}")
    } else {
        let number = format!("{scaled:.1}");
        format!("{}{}", number.trim_end_matches(".0"), units[unit])
    }
}
fn elapsed(seconds: f64) -> String {
    let seconds = seconds.max(0.0) as u64;
    let (hours, minutes, seconds) = (seconds / 3600, seconds / 60 % 60, seconds % 60);
    if hours > 0 {
        format!("{hours}h {minutes:02}m {seconds:02}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds:02}s")
    } else {
        format!("{seconds}s")
    }
}
fn script_locations(value: &serde_json::Value, path: &str, out: &mut Vec<String>) {
    match value {
        serde_json::Value::Object(map) => {
            if let (Some(x), Some(z)) = (
                map.get("x").and_then(serde_json::Value::as_i64),
                map.get("z").and_then(serde_json::Value::as_i64),
            ) {
                let y = map.get("y").map(|y| format!(" Y={y}")).unwrap_or_default();
                out.push(format!("{path} X={x} Z={z}{y}"));
            } else {
                for (key, child) in map {
                    let path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{path}.{key}")
                    };
                    script_locations(child, &path, out);
                }
            }
        }
        serde_json::Value::Array(values) => {
            for (index, child) in values.iter().enumerate() {
                script_locations(child, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}
fn hit_details(hit: &Hit) -> (String, String, u16) {
    let mut locations = Vec::new();
    if let Some(data) = &hit.data {
        script_locations(data, "", &mut locations);
    }
    // Explicit script locations describe the accepted result; native evidence
    // may contain independent witnesses unrelated to the script's final choice.
    if locations.is_empty() {
        for site in hit.evidence.iter().flat_map(|e| &e.sites) {
            let label = format!(
                "{:?} X={} Z={}",
                site.structure, site.position.x, site.position.z
            );
            if !locations.contains(&label) {
                locations.push(label);
            }
        }
    }
    let mut details = Vec::new();
    if let Some(data) = hit.data.as_ref().and_then(serde_json::Value::as_object) {
        for (key, value) in data {
            if key == "estimated_base_y" {
                if let Some(y) = value.as_f64() {
                    details.push(format!("base Y≈{y:.1}"));
                }
            } else if key == "height_validation"
                && value.as_str() == Some("climate_depth_estimate_not_generated_surface")
            {
                details.push("height: estimate".into());
            } else if key == "height_validation"
                && value.as_str() == Some("vanilla_26.2_structure_start")
            {
                details.push("height: exact · vanilla 26.2".into());
            } else {
                let mut coordinates = Vec::new();
                script_locations(value, key, &mut coordinates);
                if coordinates.is_empty() {
                    details.push(format!("{key}: {value}"));
                }
            }
        }
    }
    let height = locations.len().max(details.len()).clamp(1, 8) as u16;
    (locations.join("\n"), details.join("\n"), height)
}
fn draw(frame: &mut Frame<'_>, screen: &Screen) {
    let area = frame.area();
    if area.width < 44 || area.height < 14 {
        frame.render_widget(
            Paragraph::new("MCFinder — enlarge terminal to 44×14\nq: stop / quit")
                .block(Block::bordered()),
            area,
        );
        return;
    }
    let slow_seeds: Vec<_> = screen
        .state
        .active_seeds
        .iter()
        .filter(|s| s.elapsed_seconds >= 1.0)
        .collect();
    let visible_seeds = slow_seeds
        .len()
        .min(area.height.saturating_sub(17) as usize);
    let progress_height = if visible_seeds == 0 {
        0
    } else {
        visible_seeds as u16 + 2
    };
    let layout = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(4),
        Constraint::Length(3),
        Constraint::Length(progress_height),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(area);
    let title = Line::from(vec![
        Span::styled(
            " MCFinder ",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(format!(
            "Java {} · {} · {}",
            screen.version, screen.status, screen.state.phase
        )),
    ]);
    frame.render_widget(
        Paragraph::new(vec![title, Line::raw(&screen.hardware)])
            .block(Block::default().borders(Borders::BOTTOM)),
        layout[0],
    );
    let stats = Layout::horizontal([
        Constraint::Percentage(34),
        Constraint::Percentage(33),
        Constraint::Percentage(33),
    ])
    .split(layout[1]);
    frame.render_widget(
        Paragraph::new(format!(
            "{}\nMatched: {}",
            screen.state.results_emitted, screen.state.matches_found
        ))
        .block(Block::bordered().title(" Results "))
        .style(Style::default().fg(Color::Green)),
        stats[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} chunks/s\n{} world starts/s",
            compact(screen.state.chunks_per_second),
            compact(screen.state.seeds_per_second)
        ))
        .block(Block::bordered().title(" Speed · last second ")),
        stats[1],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} bases\n{} worlds",
            compact(screen.state.bases_tested as f64),
            compact(screen.state.seeds_evaluated as f64)
        ))
        .block(Block::bordered().title(" Started ")),
        stats[2],
    );
    let completed = screen
        .state
        .completed_before_run
        .saturating_add(screen.state.bases_committed);
    let progress_label = format!(
        "{completed}/{} · {}",
        screen.state.total_bases,
        elapsed(screen.state.elapsed_seconds)
    );
    let progress_width = (progress_label.chars().count() + 4).max(13) as u16;
    let middle = Layout::horizontal([
        Constraint::Length(progress_width.min(layout[2].width.saturating_sub(12))),
        Constraint::Min(12),
    ])
    .split(layout[2]);
    let ratio = if screen.state.total_bases == 0 {
        0.0
    } else {
        (completed as f64 / screen.state.total_bases as f64).clamp(0.0, 1.0)
    };
    frame.render_widget(
        Gauge::default()
            .block(Block::bordered().title(" Committed "))
            .ratio(ratio)
            .label(progress_label)
            .gauge_style(Style::default().fg(Color::Cyan)),
        middle[0],
    );
    let chart_width = usize::from(middle[1].width.saturating_sub(2));
    let rates: Vec<_> = (0..chart_width)
        .map(|x| {
            let index =
                x * screen.rates.len().saturating_sub(1) / chart_width.saturating_sub(1).max(1);
            screen.rates.get(index).copied().unwrap_or(0)
        })
        .collect();
    frame.render_widget(
        Sparkline::default()
            .block(Block::bordered().title(format!(
                " Chunks/s · {} ",
                compact(screen.state.chunks_per_second)
            )))
            .data(&rates)
            .style(Style::default().fg(Color::Cyan)),
        middle[1],
    );
    if visible_seeds > 0 {
        let block = Block::bordered().title(format!(
            " Seed progress · {}/{} active · regions ",
            visible_seeds,
            slow_seeds.len()
        ));
        let inner = block.inner(layout[3]);
        frame.render_widget(block, layout[3]);
        for (i, seed) in slow_seeds.iter().take(visible_seeds).enumerate() {
            let ratio =
                (seed.regions_checked as f64 / seed.total_regions.max(1) as f64).clamp(0.0, 1.0);
            let label = format!(
                "{} · {}/{} · {:.4}% · R({},{}) · {}",
                seed.seed,
                compact(seed.regions_checked as f64),
                compact(seed.total_regions as f64),
                ratio * 100.0,
                seed.region_x,
                seed.region_z,
                elapsed(seed.elapsed_seconds)
            );
            let row = ratatui::layout::Rect::new(inner.x, inner.y + i as u16, inner.width, 1);
            frame.render_widget(
                Gauge::default()
                    .ratio(ratio)
                    .label(label)
                    .gauge_style(Style::default().fg(Color::Cyan)),
                row,
            );
        }
    }
    let rows = screen.hits.iter().skip(screen.scroll).map(|hit| {
        let (positions, details, height) = hit_details(hit);
        Row::new([hit.seed.clone(), positions, details]).height(height)
    });
    let table = Table::new(
        rows,
        [
            Constraint::Length(21),
            Constraint::Fill(2),
            Constraint::Fill(1),
        ],
    )
    .header(
        Row::new(["Seed", "Locations · X,Z", "Details"])
            .style(Style::default().add_modifier(Modifier::BOLD)),
    )
    .block(Block::bordered().title(" Latest results · retained 200 · full output via --output "));
    frame.render_widget(table, layout[4]);
    let footer = screen.error.as_deref().unwrap_or(
        "Space: pause/resume   ↑/↓: history   q: stop/save   Enter/q after finish: exit",
    );
    frame.render_widget(
        Paragraph::new(footer).style(Style::default().fg(if screen.error.is_some() {
            Color::Red
        } else {
            Color::DarkGray
        })),
        layout[5],
    );
}
fn drain(receiver: &Receiver<Event>, screen: &mut Screen) {
    for _ in 0..512 {
        match receiver.try_recv() {
            Ok(event) => screen.apply(event),
            Err(_) => break,
        }
    }
}
struct TuiJob {
    receiver: Option<Receiver<Event>>,
    thread: Option<std::thread::JoinHandle<engine::Result<Summary>>>,
    control: Control,
}
impl TuiJob {
    fn finished(&self) -> bool {
        self.thread.as_ref().is_some_and(|t| t.is_finished())
    }
}
impl Drop for TuiJob {
    fn drop(&mut self) {
        self.control.cancel();
        self.receiver.take(); // unblock an event producer before joining it
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}
fn tui(settings: Settings, options: RunOptions, control: Control) -> engine::Result<Summary> {
    if !io::stdin().is_terminal() || !io::stderr().is_terminal() {
        return Err("TUI requires an interactive terminal; use --ui backend or --ui plain".into());
    }
    enable_raw_mode()?;
    let _guard = TerminalGuard;
    execute!(io::stderr(), EnterAlternateScreen, crossterm::cursor::Hide)?;
    let mut terminal = Terminal::new(CrosstermBackend::new(io::stderr()))?;
    let (sender, receiver) = mpsc::sync_channel(256);
    let observer: Observer = Arc::new(move |event| sender.send(event).map_err(|e| e.to_string()));
    let c = control.clone();
    let thread = std::thread::spawn(move || engine::run(settings, options, c, observer));
    let mut job = TuiJob {
        receiver: Some(receiver),
        thread: Some(thread),
        control: control.clone(),
    };
    let mut screen = Screen::default();
    let mut quit_when_finished = false;
    loop {
        drain(job.receiver.as_ref().unwrap(), &mut screen);
        if job.finished() && quit_when_finished {
            break;
        }
        if !job.finished() {
            screen.state.phase = control.snapshot().phase;
        }
        terminal.draw(|frame| draw(frame, &screen))?;
        if event::poll(Duration::from_millis(100))?
            && let TerminalEvent::Key(key) = event::read()?
        {
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Enter if job.finished() => break,
                KeyCode::Char('q') | KeyCode::Esc => {
                    if job.finished() {
                        break;
                    }
                    quit_when_finished = true;
                    control.cancel();
                    screen.status = "Stopping / saving checkpoint".into();
                }
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    if job.finished() {
                        break;
                    }
                    quit_when_finished = true;
                    control.cancel();
                }
                KeyCode::Char(' ') if !job.finished() => {
                    if control.is_paused() {
                        control.resume();
                    } else {
                        control.pause();
                    }
                }
                KeyCode::Down => {
                    screen.scroll = (screen.scroll + 1).min(screen.hits.len().saturating_sub(1))
                }
                KeyCode::Up => screen.scroll = screen.scroll.saturating_sub(1),
                _ => {}
            }
        }
    }
    job.thread
        .take()
        .unwrap()
        .join()
        .map_err(|_| "Search worker panicked")?
}
#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    #[test]
    fn slow_seed_bars_render_and_fit_small_terminals() {
        let mut screen = Screen::default();
        screen.state.active_seeds = (0..12)
            .map(|i| crate::events::SeedProgress {
                seed: (4743858 + i).to_string(),
                seed_bits: (4743858 + i).to_string(),
                regions_checked: 1_000_000,
                total_regions: 4_000_000,
                region_x: -19,
                region_z: 20,
                elapsed_seconds: 65.0,
            })
            .collect();
        for (w, h) in [(44, 14), (80, 20), (140, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &screen)).unwrap();
            let text: String = terminal
                .backend()
                .buffer()
                .content
                .iter()
                .map(|c| c.symbol())
                .collect();
            if h >= 20 {
                assert!(text.contains("Seed progress"));
                assert!(text.contains("25.0000%"));
            }
            if h >= 40 {
                assert!(text.contains("4743858"));
                assert!(text.contains("4743869"));
            }
        }
    }
    #[test]
    fn chart_and_label_use_chunk_rate() {
        let mut screen = Screen::default();
        screen.apply(Event::Progress {
            state: Snapshot {
                chunks_per_second: 1_260_000.0,
                bases_per_second: 42.0,
                ..Snapshot::default()
            },
        });
        assert_eq!(screen.rates.back(), Some(&1_260_000));
        let mut terminal = Terminal::new(TestBackend::new(120, 24)).unwrap();
        terminal.draw(|f| draw(f, &screen)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|c| c.symbol())
            .collect();
        assert!(text.contains("Chunks/s · 1.3M"));
        assert!(text.contains("1.3M chunks/s"));
    }
    #[test]
    fn readable_metrics() {
        assert_eq!(compact(999.0), "999");
        assert_eq!(compact(245966.0), "246K");
        assert_eq!(compact(97962668.0), "98M");
        assert_eq!(compact(1e12), "1T");
        assert_eq!(elapsed(59.9), "59s");
        assert_eq!(elapsed(415.0), "6m 55s");
        assert_eq!(elapsed(3661.0), "1h 01m 01s");
    }
    #[test]
    fn script_coordinates_and_estimated_height_are_visible() {
        let hit: Hit = serde_json::from_value(serde_json::json!({
            "seed": "29276", "seed_bits": "29276", "version": "26.2",
            "validation": "biome_candidate", "experimental": true,
            "spawn": null, "evidence": [],
            "data": {"mansion": {"x": -4448, "z": -3312},
                "estimated_base_y": 155.69357299804688,
                "height_validation": "climate_depth_estimate_not_generated_surface"}
        }))
        .unwrap();
        let mut screen = Screen::default();
        screen.hits.push_back(hit);
        let mut terminal = Terminal::new(TestBackend::new(100, 24)).unwrap();
        terminal.draw(|f| draw(f, &screen)).unwrap();
        let text: String = terminal
            .backend()
            .buffer()
            .content
            .iter()
            .map(|cell| cell.symbol())
            .collect();
        assert!(text.contains("mansion X=-4448 Z=-3312"));
        assert!(text.contains("base Y≈155.7"));
        assert!(text.contains("height: estimate"));
    }
    #[test]
    fn nested_script_positions_include_array_indices_and_exact_y() {
        let mut locations = Vec::new();
        script_locations(
            &serde_json::json!({"sites": [{"x": 12, "y": 180, "z": -3}]}),
            "",
            &mut locations,
        );
        assert_eq!(locations, ["sites[0] X=12 Z=-3 Y=180"]);
    }
    #[test]
    fn renders_small_and_full_terminals() {
        for (w, h) in [(20, 8), (44, 14), (100, 30)] {
            let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
            terminal.draw(|f| draw(f, &Screen::default())).unwrap();
            assert!(
                terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .any(|c| c.symbol().contains('M'))
            );
        }
    }
}
