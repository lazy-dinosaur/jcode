use super::*;
use crate::tui::scratchpad_terminal::{ScratchpadSizeMode, ScratchpadTerminal};
use crossterm::event::KeyEvent;

impl App {
    pub(super) fn open_scratchpad_terminal(
        &mut self,
        title: impl Into<String>,
        program: impl Into<String>,
        args: Vec<String>,
        cwd: PathBuf,
    ) {
        let title = title.into();
        let program = program.into();
        if let Some(existing_cell) = self.scratchpad_terminal.as_ref() {
            let mut existing = existing_cell.borrow_mut();
            if !existing.is_exited() {
                existing.show();
                let existing_title = existing.title().to_string();
                drop(existing);
                self.set_status_notice(format!("Scratchpad → {existing_title}"));
                return;
            }
        }

        match ScratchpadTerminal::spawn(title.clone(), program.clone(), args, cwd) {
            Ok(scratchpad) => {
                self.scratchpad_terminal = Some(RefCell::new(scratchpad));
                self.set_status_notice(format!("Scratchpad opened: {title}"));
            }
            Err(error) => {
                self.push_display_message(DisplayMessage::error(format!(
                    "Failed to open `{program}` scratchpad: {error}"
                )));
            }
        }
    }

    pub(super) fn toggle_scratchpad_terminal(&mut self) -> bool {
        let Some(scratchpad_cell) = self.scratchpad_terminal.as_ref() else {
            self.push_display_message(DisplayMessage::system(
                "No active scratchpad. Use `/nvim` or `/lazygit` first.".to_string(),
            ));
            return true;
        };

        let mut clear_after = false;
        {
            let mut scratchpad = scratchpad_cell.borrow_mut();
            if scratchpad.is_exited() {
                clear_after = true;
            } else {
                scratchpad.toggle_visible();
            }
        }

        if clear_after {
            self.scratchpad_terminal = None;
            self.push_display_message(DisplayMessage::system(
                "No active scratchpad. Use `/nvim` or `/lazygit` first.".to_string(),
            ));
        }
        true
    }

    pub(super) fn set_scratchpad_size_mode(&mut self, mode: ScratchpadSizeMode) -> bool {
        let Some(scratchpad_cell) = self.scratchpad_terminal.as_ref() else {
            self.push_display_message(DisplayMessage::system(format!(
                "No active scratchpad. Use `/nvim` or `/lazygit` first, then `/scratchpad {}`.",
                mode.label()
            )));
            return true;
        };

        let mut scratchpad = scratchpad_cell.borrow_mut();
        if scratchpad.is_exited() {
            drop(scratchpad);
            self.scratchpad_terminal = None;
            self.push_display_message(DisplayMessage::system(
                "No active scratchpad. Use `/nvim` or `/lazygit` first.".to_string(),
            ));
            return true;
        }

        scratchpad.set_size_mode(mode);
        scratchpad.show();
        drop(scratchpad);
        self.set_status_notice(format!("Scratchpad size → {}", mode.label()));
        true
    }

    pub(super) fn handle_scratchpad_key(&mut self, event: KeyEvent) -> Result<bool> {
        let Some(scratchpad_cell) = self.scratchpad_terminal.as_ref() else {
            return Ok(false);
        };
        let mut scratchpad = scratchpad_cell.borrow_mut();
        if !scratchpad.is_visible() || scratchpad.is_exited() {
            return Ok(false);
        }
        match scratchpad.handle_key(event) {
            Ok(consumed) => Ok(consumed),
            Err(error) => {
                drop(scratchpad);
                self.push_display_message(DisplayMessage::error(format!(
                    "Scratchpad input failed: {error}"
                )));
                Ok(true)
            }
        }
    }

    pub(super) fn handle_scratchpad_paste(&mut self, text: &str) -> bool {
        let Some(scratchpad_cell) = self.scratchpad_terminal.as_ref() else {
            return false;
        };
        let mut scratchpad = scratchpad_cell.borrow_mut();
        if !scratchpad.is_visible() || scratchpad.is_exited() {
            return false;
        }
        if let Err(error) = scratchpad.write_paste(text) {
            drop(scratchpad);
            self.push_display_message(DisplayMessage::error(format!(
                "Scratchpad paste failed: {error}"
            )));
        }
        true
    }

    pub(super) fn poll_scratchpad_terminal(&mut self) -> bool {
        let Some(scratchpad_cell) = self.scratchpad_terminal.as_ref() else {
            return false;
        };

        let (mut needs_redraw, exit_message, remove) = {
            let mut scratchpad = scratchpad_cell.borrow_mut();
            let needs_redraw = scratchpad.poll();
            let exit_message = scratchpad.take_exit_message();
            let remove = scratchpad.is_exited() && !scratchpad.is_visible();
            (needs_redraw, exit_message, remove)
        };

        if let Some(message) = exit_message {
            self.push_display_message(DisplayMessage::system(message));
            needs_redraw = true;
        }
        if remove {
            self.scratchpad_terminal = None;
        }
        needs_redraw
    }
}
