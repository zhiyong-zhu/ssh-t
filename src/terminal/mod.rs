/// Terminal screen emulator.
/// Parses ANSI output and maintains a 2D character grid for display.
use vte::{Params, Perform};

/// A simple terminal screen that tracks visible lines.
pub struct TermScreen {
    lines: Vec<String>,
    max_lines: usize,
}

impl TermScreen {
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: vec![String::new()],
            max_lines,
        }
    }

    /// Get the last N lines for display.
    pub fn get_visible_lines(&self, height: usize) -> Vec<String> {
        let total = self.lines.len();
        if total <= height {
            self.lines.clone()
        } else {
            self.lines[total - height..].to_vec()
        }
    }

    /// Get total line count.
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Process raw bytes and update screen.
    pub fn process(&mut self, data: &[u8]) {
        let mut parser = vte::Parser::new();
        parser.advance(self, data);
    }

    /// Clear the screen.
    pub fn clear(&mut self) {
        self.lines.clear();
        self.lines.push(String::new());
    }
}

impl Perform for TermScreen {
    fn print(&mut self, c: char) {
        if let Some(last) = self.lines.last_mut() {
            last.push(c);
        }
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x0A => {
                // LF: new line
                let new_line = String::new();
                if self.lines.len() >= self.max_lines {
                    // Remove oldest line to prevent unbounded growth
                    self.lines.remove(0);
                }
                self.lines.push(new_line);
            }
            0x0D => {
                // CR: move to beginning of current line — no-op in line-based model
            }
            0x09 => {
                // HT: tab
                if let Some(last) = self.lines.last_mut() {
                    last.push_str("    ");
                }
            }
            0x08 => {
                // BS: backspace - remove last char
                if let Some(last) = self.lines.last_mut() {
                    last.pop();
                }
            }
            _ => {}
        }
    }

    fn csi_dispatch(
        &mut self,
        params: &Params,
        _intermediates: &[u8],
        _ignore: bool,
        action: char,
    ) {
        match action {
            // Erase in line
            'K' => {
                let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0);
                if mode == 2 {
                    // Erase entire line
                    if let Some(last) = self.lines.last_mut() {
                        last.clear();
                    }
                } else if mode == 1 {
                    // Erase from cursor to beginning - just clear for simplicity
                    if let Some(last) = self.lines.last_mut() {
                        last.clear();
                    }
                }
            }
            // Erase in display
            'J' => {
                let mode = params.iter().next().and_then(|p| p.first()).copied().unwrap_or(0);
                if mode == 2 || mode == 3 {
                    // Clear entire screen
                    self.lines.clear();
                    self.lines.push(String::new());
                } else if mode == 1 {
                    // Clear from cursor to beginning
                }
            }
            // Cursor up — not modeled in line-based rendering
            'A' => {}
            // Cursor down — not modeled in line-based rendering
            'B' => {}
            // Set cursor position (move to specific line)
            'H' | 'f' => {
                // CUP / HVP - for simplicity, just ignore precise positioning
            }
            // SGR (colors/styles) - ignore
            'm' => {}
            // Other CSI - ignore
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _action: u8) {}
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
}
