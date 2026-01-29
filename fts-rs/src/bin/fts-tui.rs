use std::collections::VecDeque;
use std::io::{self, BufRead};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyModifiers};
use crossterm::terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen};
use crossterm::ExecutableCommand;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};
use ratatui::Terminal;

#[derive(Default)]
struct AppState {
    logs: VecDeque<String>,
    status: String,
    total_connections: u64,
    last_error: Option<String>,
    listening: Vec<String>,
}

fn main() -> Result<()> {
    let mut child = spawn_server()?;
    let start = Instant::now();

    let (tx, rx) = mpsc::channel::<String>();
    if let Some(stdout) = child.stdout.take() {
        spawn_reader_thread(stdout, tx.clone(), "stdout");
    }
    if let Some(stderr) = child.stderr.take() {
        spawn_reader_thread(stderr, tx, "stderr");
    }

    let mut state = AppState {
        status: "RUNNING".to_string(),
        ..Default::default()
    };

    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = io::stdout();
    stdout.execute(EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut should_exit = false;
    while !should_exit {
        drain_logs(&rx, &mut state);

        if let Some(status) = child.try_wait()? {
            state.status = format!("EXITED ({})", status);
            should_exit = true;
        }

        terminal.draw(|f| {
            let size = f.size();
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([Constraint::Length(6), Constraint::Min(1)])
                .split(size);

            let uptime = format!("{:.0}s", start.elapsed().as_secs_f64());
            let listening = if state.listening.is_empty() {
                "(not yet)".to_string()
            } else {
                state.listening.join(" | ")
            };
            let header_lines = vec![
                Line::from(vec![
                    Span::styled("Status: ", Style::default().fg(Color::Yellow)),
                    Span::raw(&state.status),
                    Span::raw("    "),
                    Span::styled("Uptime: ", Style::default().fg(Color::Yellow)),
                    Span::raw(uptime),
                ]),
                Line::from(vec![
                    Span::styled("Connections: ", Style::default().fg(Color::Yellow)),
                    Span::raw(state.total_connections.to_string()),
                ]),
                Line::from(vec![
                    Span::styled("Listening: ", Style::default().fg(Color::Yellow)),
                    Span::raw(listening),
                ]),
                Line::from(vec![
                    Span::styled("Last error: ", Style::default().fg(Color::Yellow)),
                    Span::raw(state.last_error.clone().unwrap_or_else(|| "(none)".to_string())),
                ]),
                Line::from("Press q or Ctrl+C to quit"),
            ];
            let header = Paragraph::new(header_lines)
                .block(Block::default().borders(Borders::ALL).title("fts-rs manager"))
                .wrap(Wrap { trim: true });
            f.render_widget(header, chunks[0]);

            let log_lines: Vec<Line> = state
                .logs
                .iter()
                .rev()
                .take(200)
                .rev()
                .map(|l| Line::raw(l.clone()))
                .collect();
            let logs = Paragraph::new(log_lines)
                .block(Block::default().borders(Borders::ALL).title("logs"))
                .wrap(Wrap { trim: false });
            f.render_widget(logs, chunks[1]);
        })?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.code == KeyCode::Char('q')
                    || (key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL))
                {
                    should_exit = true;
                }
            }
        }
    }

    let _ = shutdown_child(&mut child);

    disable_raw_mode().ok();
    let mut stdout = io::stdout();
    stdout.execute(LeaveAlternateScreen).ok();

    Ok(())
}

fn spawn_server() -> Result<Child> {
    let exe = std::env::current_exe().context("current_exe")?;
    let exe_dir = exe.parent().context("exe parent")?;
    let server_exe = exe_dir.join("fts-rs.exe");

    Command::new(server_exe)
        .current_dir(".")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("spawn fts-rs")
}

fn spawn_reader_thread<R: io::Read + Send + 'static>(reader: R, tx: Sender<String>, label: &str) {
    let label = label.to_string();
    thread::spawn(move || {
        let buf = io::BufReader::new(reader);
        for line in buf.lines() {
            if let Ok(line) = line {
                let _ = tx.send(format!("[{}] {}", label, line));
            }
        }
    });
}

fn drain_logs(rx: &Receiver<String>, state: &mut AppState) {
    while let Ok(line) = rx.try_recv() {
        if line.contains("New TCP connection") || line.contains("New TLS connection") {
            state.total_connections += 1;
        }
        if line.contains("listening on") {
            state.listening.push(line.clone());
        }
        if line.contains("ERROR") {
            state.last_error = Some(line.clone());
        }
        state.logs.push_back(line);
        if state.logs.len() > 500 {
            state.logs.pop_front();
        }
    }
}

fn shutdown_child(child: &mut Child) -> Result<()> {
    if child.try_wait()?.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }
    Ok(())
}
