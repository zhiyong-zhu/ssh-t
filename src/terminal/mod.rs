use vte::{Params, Perform};

/// Terminal screen emulator.
/// Parses ANSI output and maintains a visible character grid for display.
pub struct TermScreen {
    parser: vte::Parser,
    buffer: ScreenBuffer,
}

impl TermScreen {
    pub fn new(max_lines: usize) -> Self {
        Self {
            parser: vte::Parser::new(),
            buffer: ScreenBuffer::new(max_lines),
        }
    }

    /// Get the last N lines for display.
    pub fn get_visible_lines(&self, height: usize) -> Vec<String> {
        self.buffer.get_visible_lines(height)
    }

    /// Get total line count.
    pub fn line_count(&self) -> usize {
        self.buffer.line_count()
    }

    /// Process raw bytes and update screen.
    pub fn process(&mut self, data: &[u8]) {
        self.parser.advance(&mut self.buffer, data);
    }

    /// Clear the screen.
    pub fn clear(&mut self) {
        self.buffer.clear();
    }
}

struct ScreenBuffer {
    lines: Vec<Vec<char>>,
    max_lines: usize,
    cursor_row: usize,
    cursor_col: usize,
}

impl ScreenBuffer {
    fn new(max_lines: usize) -> Self {
        Self {
            lines: vec![Vec::new()],
            max_lines,
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    fn get_visible_lines(&self, height: usize) -> Vec<String> {
        let total = self.lines.len();
        let lines = if total <= height {
            &self.lines[..]
        } else {
            &self.lines[total - height..]
        };
        lines.iter().map(|line| line.iter().collect()).collect()
    }

    fn line_count(&self) -> usize {
        self.lines.len()
    }

    fn clear(&mut self) {
        self.lines.clear();
        self.lines.push(Vec::new());
        self.cursor_row = 0;
        self.cursor_col = 0;
    }

    fn ensure_cursor_line(&mut self) {
        while self.cursor_row >= self.lines.len() {
            self.lines.push(Vec::new());
        }
        self.trim_overflow();
    }

    fn trim_overflow(&mut self) {
        while self.lines.len() > self.max_lines {
            self.lines.remove(0);
            self.cursor_row = self.cursor_row.saturating_sub(1);
        }
    }

    fn current_line_mut(&mut self) -> &mut Vec<char> {
        self.ensure_cursor_line();
        &mut self.lines[self.cursor_row]
    }

    fn pad_to_cursor(line: &mut Vec<char>, cursor_col: usize) {
        if line.len() < cursor_col {
            line.resize(cursor_col, ' ');
        }
    }

    fn move_to_next_line(&mut self) {
        self.cursor_row += 1;
        self.cursor_col = 0;
        self.ensure_cursor_line();
    }

    fn delete_chars_from_cursor(&mut self, count: usize) {
        let cursor_col = self.cursor_col;
        let line = self.current_line_mut();
        if cursor_col >= line.len() || count == 0 {
            return;
        }

        let end = (cursor_col + count).min(line.len());
        line.drain(cursor_col..end);
    }

    fn insert_spaces_at_cursor(&mut self, count: usize) {
        if count == 0 {
            return;
        }

        let cursor_col = self.cursor_col;
        let line = self.current_line_mut();
        Self::pad_to_cursor(line, cursor_col);
        line.splice(cursor_col..cursor_col, std::iter::repeat(' ').take(count));
    }

    fn erase_chars_from_cursor(&mut self, count: usize) {
        if count == 0 {
            return;
        }

        let cursor_col = self.cursor_col;
        let line = self.current_line_mut();
        Self::pad_to_cursor(line, cursor_col + count);
        for cell in line.iter_mut().skip(cursor_col).take(count) {
            *cell = ' ';
        }
    }
}

impl Perform for ScreenBuffer {
    fn print(&mut self, c: char) {
        let cursor_col = self.cursor_col;
        let line = self.current_line_mut();
        Self::pad_to_cursor(line, cursor_col);

        if cursor_col < line.len() {
            line[cursor_col] = c;
        } else {
            line.push(c);
        }

        self.cursor_col += 1;
    }

    fn execute(&mut self, byte: u8) {
        match byte {
            0x0A => self.move_to_next_line(),
            0x0D => self.cursor_col = 0,
            0x09 => {
                let next_tab_stop = ((self.cursor_col / 4) + 1) * 4;
                while self.cursor_col < next_tab_stop {
                    self.print(' ');
                }
            }
            0x08 => {
                if self.cursor_col > 0 {
                    self.cursor_col -= 1;
                    let cursor_col = self.cursor_col;
                    let line = self.current_line_mut();
                    if cursor_col < line.len() {
                        line.remove(cursor_col);
                    }
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
        let first = params
            .iter()
            .next()
            .and_then(|param| param.first())
            .copied()
            .unwrap_or(0) as usize;

        match action {
            'A' => {
                let amount = first.max(1);
                self.cursor_row = self.cursor_row.saturating_sub(amount);
            }
            'B' => {
                let amount = first.max(1);
                self.cursor_row += amount;
                self.ensure_cursor_line();
            }
            'C' => {
                self.cursor_col += first.max(1);
            }
            'D' => {
                let amount = first.max(1);
                self.cursor_col = self.cursor_col.saturating_sub(amount);
            }
            'G' => {
                self.cursor_col = first.saturating_sub(1);
            }
            'H' | 'f' => {
                let row = params
                    .iter()
                    .next()
                    .and_then(|param| param.first())
                    .copied()
                    .unwrap_or(1) as usize;
                let col = params
                    .iter()
                    .nth(1)
                    .and_then(|param| param.first())
                    .copied()
                    .unwrap_or(1) as usize;
                self.cursor_row = row.saturating_sub(1);
                self.cursor_col = col.saturating_sub(1);
                self.ensure_cursor_line();
            }
            'J' => {
                if first == 2 || first == 3 {
                    self.clear();
                }
            }
            'K' => {
                let cursor_col = self.cursor_col;
                let line = self.current_line_mut();
                match first {
                    0 => {
                        if cursor_col < line.len() {
                            line.truncate(cursor_col);
                        }
                    }
                    1 => {
                        let end = cursor_col.min(line.len());
                        for cell in &mut line[..end] {
                            *cell = ' ';
                        }
                    }
                    2 => {
                        line.clear();
                        self.cursor_col = 0;
                    }
                    _ => {}
                }
            }
            '@' => {
                self.insert_spaces_at_cursor(first.max(1));
            }
            'P' => {
                self.delete_chars_from_cursor(first.max(1));
            }
            'X' => {
                self.erase_chars_from_cursor(first.max(1));
            }
            'm' => {}
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {}
    fn esc_dispatch(&mut self, _intermediates: &[u8], _ignore: bool, _action: u8) {}
    fn hook(&mut self, _params: &Params, _intermediates: &[u8], _ignore: bool, _action: char) {}
    fn put(&mut self, _byte: u8) {}
    fn unhook(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::TermScreen;

    #[test]
    fn keeps_parser_state_across_chunks() {
        let mut screen = TermScreen::new(10);
        screen.process(b"\x1b[2");
        screen.process(b"Kabc");

        assert_eq!(screen.get_visible_lines(1), vec!["abc".to_string()]);
    }

    #[test]
    fn redraws_line_after_carriage_return_and_clear() {
        let mut screen = TermScreen::new(10);
        screen.process(b"prompt> \r");
        screen.process(b"\x1b[2Kprompt> ls");

        assert_eq!(screen.get_visible_lines(1), vec!["prompt> ls".to_string()]);
    }

    #[test]
    fn preserves_current_input_on_same_line() {
        let mut screen = TermScreen::new(10);
        screen.process(b"user@host:~$ ");
        screen.process(b"ls");

        assert_eq!(
            screen.get_visible_lines(1),
            vec!["user@host:~$ ls".to_string()]
        );
    }

    #[test]
    fn supports_delete_char_from_cursor() {
        let mut screen = TermScreen::new(10);
        screen.process(b"abcd");
        screen.process(b"\x1b[2D");
        screen.process(b"\x1b[P");

        assert_eq!(screen.get_visible_lines(1), vec!["abd".to_string()]);
    }

    #[test]
    fn supports_insert_char_in_middle() {
        let mut screen = TermScreen::new(10);
        screen.process(b"abc");
        screen.process(b"\x1b[2D");
        screen.process(b"\x1b[@");
        screen.process(b"X");

        assert_eq!(screen.get_visible_lines(1), vec!["aXbc".to_string()]);
    }

    #[test]
    fn keeps_cjk_input_visible() {
        let mut screen = TermScreen::new(10);
        screen.process("中文命令".as_bytes());

        assert_eq!(screen.get_visible_lines(1), vec!["中文命令".to_string()]);
    }
}
