use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use portable_pty::{Child, CommandBuilder, MasterPty, PtySize, native_pty_system};
use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    prelude::*,
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};
use std::thread;
use tui_term::widget::PseudoTerminal;

const DEFAULT_ROWS: u16 = 24;
const DEFAULT_COLS: u16 = 80;
const SCROLLBACK_LINES: usize = 2_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScratchpadSizeMode {
    Full,
    Large,
    Compact,
    Panel,
}

impl ScratchpadSizeMode {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "full" | "fullscreen" | "max" | "maximize" => Some(Self::Full),
            "large" | "big" => Some(Self::Large),
            "compact" | "small" => Some(Self::Compact),
            "panel" | "side" | "right" => Some(Self::Panel),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Large => "large",
            Self::Compact => "compact",
            Self::Panel => "panel",
        }
    }
}

#[derive(Debug)]
enum ScratchpadEvent {
    Output(Vec<u8>),
    ReaderEnded,
}

pub struct ScratchpadTerminal {
    title: String,
    program: String,
    args: Vec<String>,
    cwd: PathBuf,
    parser: vt100::Parser,
    master: Box<dyn MasterPty + Send>,
    child: Box<dyn Child + Send + Sync>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    rx: Receiver<ScratchpadEvent>,
    visible: bool,
    size_mode: ScratchpadSizeMode,
    exited: bool,
    exit_message: Option<String>,
    last_size: PtySize,
}

impl ScratchpadTerminal {
    pub(crate) fn spawn(
        title: impl Into<String>,
        program: impl Into<String>,
        args: Vec<String>,
        cwd: PathBuf,
    ) -> anyhow::Result<Self> {
        let title = title.into();
        let program = program.into();
        let size = PtySize {
            rows: DEFAULT_ROWS,
            cols: DEFAULT_COLS,
            pixel_width: 0,
            pixel_height: 0,
        };
        let pty_system = native_pty_system();
        let pair = pty_system.openpty(size)?;

        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&args);
        cmd.cwd(cwd.as_os_str());
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let writer = Arc::new(Mutex::new(pair.master.take_writer()?));
        let mut reader = pair.master.try_clone_reader()?;
        let (tx, rx) = mpsc::channel();
        thread::Builder::new()
            .name(format!("jcode-scratchpad-{program}"))
            .spawn(move || {
                let mut buf = [0_u8; 8192];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break,
                        Ok(n) => {
                            if tx.send(ScratchpadEvent::Output(buf[..n].to_vec())).is_err() {
                                break;
                            }
                        }
                        Err(_) => break,
                    }
                }
                let _ = tx.send(ScratchpadEvent::ReaderEnded);
            })?;

        Ok(Self {
            title,
            program,
            args,
            cwd,
            parser: vt100::Parser::new(size.rows, size.cols, SCROLLBACK_LINES),
            master: pair.master,
            child,
            writer,
            rx,
            visible: true,
            size_mode: ScratchpadSizeMode::Full,
            exited: false,
            exit_message: None,
            last_size: size,
        })
    }

    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn is_visible(&self) -> bool {
        self.visible
    }

    pub(crate) fn is_exited(&self) -> bool {
        self.exited
    }

    pub(crate) fn show(&mut self) {
        if !self.exited {
            self.visible = true;
        }
    }

    pub(crate) fn hide(&mut self) {
        self.visible = false;
    }

    pub(crate) fn toggle_visible(&mut self) {
        if !self.exited {
            self.visible = !self.visible;
        }
    }

    pub(crate) fn set_size_mode(&mut self, mode: ScratchpadSizeMode) {
        self.size_mode = mode;
    }

    pub(crate) fn kill(&mut self) {
        if !self.exited {
            let _ = self.child.kill();
        }
    }

    pub(crate) fn poll(&mut self) -> bool {
        let mut changed = false;
        while let Ok(event) = self.rx.try_recv() {
            match event {
                ScratchpadEvent::Output(bytes) => {
                    self.parser.process(&bytes);
                    changed = true;
                }
                ScratchpadEvent::ReaderEnded => {
                    changed = true;
                }
            }
        }

        if !self.exited {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    self.exited = true;
                    self.visible = false;
                    self.exit_message = Some(format!(
                        "`{}` scratchpad exited with {}.",
                        self.program, status
                    ));
                    changed = true;
                }
                Ok(None) => {}
                Err(error) => {
                    self.exited = true;
                    self.visible = false;
                    self.exit_message = Some(format!(
                        "`{}` scratchpad status check failed: {}",
                        self.program, error
                    ));
                    changed = true;
                }
            }
        }

        changed
    }

    pub(crate) fn take_exit_message(&mut self) -> Option<String> {
        self.exit_message.take()
    }

    pub(crate) fn write_paste(&mut self, text: &str) -> anyhow::Result<()> {
        self.write_bytes(text.as_bytes())
    }

    pub(crate) fn handle_key(&mut self, event: KeyEvent) -> anyhow::Result<bool> {
        if event.modifiers == KeyModifiers::CONTROL && matches!(event.code, KeyCode::Char('g')) {
            self.hide();
            return Ok(true);
        }
        if event.modifiers == KeyModifiers::CONTROL && matches!(event.code, KeyCode::Char('q')) {
            self.kill();
            self.hide();
            return Ok(true);
        }

        if let Some(bytes) = key_event_to_bytes(event) {
            self.write_bytes(&bytes)?;
            return Ok(true);
        }
        Ok(false)
    }

    fn write_bytes(&mut self, bytes: &[u8]) -> anyhow::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| anyhow::anyhow!("scratchpad writer lock poisoned"))?;
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    fn resize(&mut self, rows: u16, cols: u16) {
        let rows = rows.max(1);
        let cols = cols.max(1);
        if self.last_size.rows == rows && self.last_size.cols == cols {
            return;
        }
        let size = PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        };
        let _ = self.master.resize(size);
        self.parser.screen_mut().set_size(rows, cols);
        self.last_size = size;
    }

    pub(crate) fn render(&mut self, frame: &mut Frame, area: Rect) {
        if !self.visible {
            return;
        }
        let outer = scratchpad_rect(area, self.size_mode);
        if outer.width < 8 || outer.height < 5 {
            return;
        }

        frame.render_widget(Clear, outer);
        let title = format!(
            " {} · {}{} ",
            self.title,
            self.program,
            if self.args.is_empty() {
                String::new()
            } else {
                format!(" {}", self.args.join(" "))
            }
        );
        let footer = format!(
            " {} · cwd: {} · Ctrl+G hide · Ctrl+Q kill ",
            self.size_mode.label(),
            compact_path(&self.cwd)
        );
        let block = Block::default()
            .title(Span::styled(
                title,
                Style::default()
                    .fg(Color::Rgb(230, 230, 240))
                    .add_modifier(Modifier::BOLD),
            ))
            .title_bottom(Span::styled(
                footer,
                Style::default().fg(Color::Rgb(150, 150, 165)),
            ))
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Rgb(100, 100, 130)))
            .style(Style::default().bg(Color::Rgb(8, 9, 14)));
        let inner = block.inner(outer);
        frame.render_widget(block, outer);

        if inner.height < 1 || inner.width < 1 {
            return;
        }
        self.resize(inner.height, inner.width);
        let terminal = PseudoTerminal::new(self.parser.screen()).style(
            Style::default()
                .fg(Color::Rgb(220, 220, 230))
                .bg(Color::Rgb(8, 9, 14)),
        );
        frame.render_widget(terminal, inner);

        if self.exited {
            let msg = Paragraph::new("process exited")
                .style(Style::default().fg(Color::Yellow).bg(Color::Rgb(8, 9, 14)));
            frame.render_widget(msg, inner);
        }
    }
}

impl Drop for ScratchpadTerminal {
    fn drop(&mut self) {
        if !self.exited {
            let _ = self.child.kill();
        }
    }
}

fn scratchpad_rect(area: Rect, mode: ScratchpadSizeMode) -> Rect {
    match mode {
        ScratchpadSizeMode::Full => {
            return Rect {
                x: area.x.saturating_add(1),
                y: area.y.saturating_add(1),
                width: area.width.saturating_sub(2).max(1),
                height: area.height.saturating_sub(2).max(1),
            };
        }
        ScratchpadSizeMode::Panel => {
            let panel_width = (area.width.saturating_mul(55) / 100)
                .max(40)
                .min(area.width);
            return Rect {
                x: area.x + area.width.saturating_sub(panel_width),
                y: area.y.saturating_add(1),
                width: panel_width,
                height: area.height.saturating_sub(2).max(1),
            };
        }
        ScratchpadSizeMode::Large | ScratchpadSizeMode::Compact => {}
    }

    let (width_pct, height_pct) = match mode {
        ScratchpadSizeMode::Large => (96, 94),
        ScratchpadSizeMode::Compact => {
            if area.width >= 140 {
                (78, 82)
            } else {
                (92, 88)
            }
        }
        ScratchpadSizeMode::Full | ScratchpadSizeMode::Panel => unreachable!(),
    };
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - height_pct) / 2),
            Constraint::Percentage(height_pct),
            Constraint::Percentage((100 - height_pct) / 2),
        ])
        .split(area);
    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - width_pct) / 2),
            Constraint::Percentage(width_pct),
            Constraint::Percentage((100 - width_pct) / 2),
        ])
        .split(vertical[1])[1]
}

fn compact_path(path: &Path) -> String {
    let text = path.display().to_string();
    if let Ok(home) = std::env::var("HOME")
        && let Some(rest) = text.strip_prefix(&home)
    {
        return format!("~{}", rest);
    }
    text
}

fn key_event_to_bytes(event: KeyEvent) -> Option<Vec<u8>> {
    let mut out = Vec::new();
    if event.modifiers.contains(KeyModifiers::ALT) {
        out.push(0x1b);
    }

    match event.code {
        KeyCode::Char(c) => {
            if event.modifiers.contains(KeyModifiers::CONTROL) {
                let lower = c.to_ascii_lowercase();
                if lower == ' ' {
                    out.push(0);
                } else if lower.is_ascii_alphabetic() {
                    out.push((lower as u8) & 0x1f);
                } else {
                    return None;
                }
            } else {
                let mut buf = [0_u8; 4];
                out.extend_from_slice(c.encode_utf8(&mut buf).as_bytes());
            }
        }
        KeyCode::Enter => out.push(b'\r'),
        KeyCode::Tab => out.push(b'\t'),
        KeyCode::Backspace => out.push(0x7f),
        KeyCode::Esc => out.push(0x1b),
        KeyCode::Left => out.extend_from_slice(b"\x1b[D"),
        KeyCode::Right => out.extend_from_slice(b"\x1b[C"),
        KeyCode::Up => out.extend_from_slice(b"\x1b[A"),
        KeyCode::Down => out.extend_from_slice(b"\x1b[B"),
        KeyCode::Home => out.extend_from_slice(b"\x1b[H"),
        KeyCode::End => out.extend_from_slice(b"\x1b[F"),
        KeyCode::PageUp => out.extend_from_slice(b"\x1b[5~"),
        KeyCode::PageDown => out.extend_from_slice(b"\x1b[6~"),
        KeyCode::Delete => out.extend_from_slice(b"\x1b[3~"),
        KeyCode::Insert => out.extend_from_slice(b"\x1b[2~"),
        KeyCode::F(n) => {
            let seq = match n {
                1 => "\x1bOP",
                2 => "\x1bOQ",
                3 => "\x1bOR",
                4 => "\x1bOS",
                5 => "\x1b[15~",
                6 => "\x1b[17~",
                7 => "\x1b[18~",
                8 => "\x1b[19~",
                9 => "\x1b[20~",
                10 => "\x1b[21~",
                11 => "\x1b[23~",
                12 => "\x1b[24~",
                _ => return None,
            };
            out.extend_from_slice(seq.as_bytes());
        }
        _ => return None,
    }

    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyEventKind;

    #[test]
    fn key_event_to_bytes_maps_common_keys() {
        let key = |code, modifiers| KeyEvent::new_with_kind(code, modifiers, KeyEventKind::Press);
        assert_eq!(
            key_event_to_bytes(key(KeyCode::Char('a'), KeyModifiers::CONTROL)).unwrap(),
            vec![1]
        );
        assert_eq!(
            key_event_to_bytes(key(KeyCode::Enter, KeyModifiers::empty())).unwrap(),
            b"\r".to_vec()
        );
        assert_eq!(
            key_event_to_bytes(key(KeyCode::Left, KeyModifiers::empty())).unwrap(),
            b"\x1b[D".to_vec()
        );
        assert_eq!(
            key_event_to_bytes(key(KeyCode::Char('é'), KeyModifiers::empty())).unwrap(),
            "é".as_bytes().to_vec()
        );
    }
}
