//! The box of the popup `message`: a title, the message and an OK
//! button, in the middle of the popup.
//!
//! The box is the same as the message box of herdr-worktrunk.

use std::io::{self, IsTerminal, Read, Write};
use std::process::{Command, Stdio};

const DEFAULT_ROWS: usize = 12;
const DEFAULT_COLUMNS: usize = 72;
/// The space on each side of the message.
const SIDE_MARGIN: usize = 2;

const CLEAR_SCREEN: &str = "\x1b[2J\x1b[H";
const HIDE_CURSOR: &str = "\x1b[?25l";
const SHOW_CURSOR: &str = "\x1b[?25h";
const BOLD: &str = "\x1b[1m";
const REVERSE: &str = "\x1b[7m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

const OK_BUTTON: &str = "[ OK ]";
const HINT: &str = "any key closes";

/// Run `stty` on the terminal of the popup, and return its output.
fn stty(arguments: &[&str]) -> Option<String> {
    let output = Command::new("stty")
        .args(arguments)
        .stdin(Stdio::inherit())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_owned())
}

/// The size of the terminal: rows, then columns.
fn terminal_size() -> (usize, usize) {
    let size = stty(&["size"]).unwrap_or_default();
    let mut numbers = size.split_whitespace().map(str::parse::<usize>);
    match (numbers.next(), numbers.next()) {
        (Some(Ok(rows)), Some(Ok(columns))) if rows > 0 && columns > 0 => (rows, columns),
        _ => (DEFAULT_ROWS, DEFAULT_COLUMNS),
    }
}

/// Cut `text` into lines of `width` characters or less, at the spaces.
fn wrap(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    for paragraph in text.lines() {
        let mut line = String::new();
        for word in paragraph.split_whitespace() {
            let needed = line.chars().count() + word.chars().count() + 1;
            if !line.is_empty() && needed > width {
                lines.push(std::mem::take(&mut line));
            }
            if !line.is_empty() {
                line.push(' ');
            }
            line.push_str(word);
        }
        lines.push(line);
    }
    lines
}

/// Put `text` in the middle of a line of `width` characters.
fn centered(text: &str, visible_length: usize, width: usize) -> String {
    let padding = width.saturating_sub(visible_length) / 2;
    format!("{}{text}", " ".repeat(padding))
}

/// The terminal in raw mode: a key arrives without Enter, and without
/// echo. The terminal comes back to its mode when the value is dropped.
struct RawMode {
    saved: Option<String>,
}

impl RawMode {
    fn start() -> Self {
        let saved = stty(&["-g"]);
        let _ = stty(&["raw", "-echo"]);
        RawMode { saved }
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        if let Some(saved) = &self.saved {
            let _ = stty(&[saved.as_str()]);
        }
        let mut stdout = io::stdout();
        // The cursor is only for the user: a failed write has no effect.
        let _ = stdout.write_all(SHOW_CURSOR.as_bytes());
        let _ = stdout.flush();
    }
}

/// Draw the box: the title, the message, the OK button and the hint.
fn draw(title: &str, message: &str) {
    let (rows, columns) = terminal_size();
    let text_width = columns.saturating_sub(2 * SIDE_MARGIN).max(1);
    let message_lines = wrap(message, text_width);
    // Title, blank line, message, blank line, button, blank line, hint.
    let content_rows = message_lines.len() + 6;
    let top_padding = rows.saturating_sub(content_rows) / 2;

    let mut screen = String::new();
    screen.push_str(CLEAR_SCREEN);
    screen.push_str(HIDE_CURSOR);
    screen.push_str(&"\r\n".repeat(top_padding));
    let title_text = format!("{BOLD}{title}{RESET}");
    screen.push_str(&centered(&title_text, title.chars().count(), columns));
    screen.push_str("\r\n\r\n");
    for line in &message_lines {
        screen.push_str(&centered(line, line.chars().count(), columns));
        screen.push_str("\r\n");
    }
    screen.push_str("\r\n");
    let button_text = format!("{REVERSE}{OK_BUTTON}{RESET}");
    screen.push_str(&centered(&button_text, OK_BUTTON.len(), columns));
    screen.push_str("\r\n\r\n");
    let hint_text = format!("{DIM}{HINT}{RESET}");
    screen.push_str(&centered(&hint_text, HINT.len(), columns));
    let mut stdout = io::stdout();
    // The box is only for the user: a failed write has no other effect.
    let _ = stdout.write_all(screen.as_bytes());
    let _ = stdout.flush();
}

/// Replace each control character of `text` with a space, except the
/// line breaks. The message can come from herdr: an escape sequence must
/// not change the terminal of the popup.
fn printable(text: &str) -> String {
    text.chars()
        .map(|character| {
            if character != '\n' && character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect()
}

/// Show `title` and `message` with an OK button, and wait for a key.
///
/// Outside a terminal, print the message on stderr.
pub fn show_message(title: &str, message: &str) {
    let is_interactive = io::stdin().is_terminal() && io::stdout().is_terminal();
    if !is_interactive {
        eprintln!("{title}: {message}");
        return;
    }
    let message = printable(message);
    let _raw = RawMode::start();
    draw(title, &message);
    // Any key closes the box: the key itself has no meaning.
    let mut bytes = [0_u8; 8];
    let _ = io::stdin().read(&mut bytes);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wrap_cuts_the_lines_at_the_spaces() {
        let lines = wrap("Send a message to the agent.", 16);

        assert_eq!(lines, ["Send a message", "to the agent."]);
    }

    #[test]
    fn wrap_keeps_a_word_longer_than_the_width() {
        let lines = wrap("/Users/thomas/Workspace/a-very-long-folder", 10);

        assert_eq!(lines, ["/Users/thomas/Workspace/a-very-long-folder"]);
    }

    #[test]
    fn printable_removes_the_escape_sequences() {
        let text = printable("red \x1b[31mtext\r\nnext line");

        assert_eq!(text, "red  [31mtext \nnext line");
    }

    #[test]
    fn centered_puts_the_text_in_the_middle() {
        assert_eq!(centered("OK", 2, 10), "    OK");
    }
}
