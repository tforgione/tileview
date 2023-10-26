//! This module contains everything related to tiles.

use std::io::{self, Read};
use std::process::{Child, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

use pty_process::blocking::Command;
use pty_process::blocking::Pty;

use unicode_width::UnicodeWidthChar;

use termion::{color, cursor, style};

use crate::{utils, Msg};

/// A helper to build tiles.
pub struct TileBuilder {
    /// The command that the tile will run.
    pub command: Option<Vec<String>>,

    /// The coordinates of the tile.
    pub coords: Option<(u16, u16)>,

    /// The top left corner of the tile.
    pub position: Option<(u16, u16)>,

    /// The size of the tile.
    pub size: Option<(u16, u16)>,

    /// The sender to communicate with the main view.
    pub sender: Option<Sender<Msg>>,
}

impl TileBuilder {
    /// Creates an empty tile builder.
    pub fn new() -> TileBuilder {
        TileBuilder {
            command: None,
            coords: None,
            position: None,
            size: None,
            sender: None,
        }
    }

    /// Sets the command of the tile.
    pub fn command(self, command: Vec<String>) -> TileBuilder {
        let mut s = self;
        s.command = Some(command);
        s
    }

    /// Sets the coordinates of the tile.
    pub fn coords(self, coords: (u16, u16)) -> TileBuilder {
        let mut s = self;
        s.coords = Some(coords);
        s
    }

    /// Sets the position of the tile.
    pub fn position(self, position: (u16, u16)) -> TileBuilder {
        let mut s = self;
        s.position = Some(position);
        s
    }

    /// Sets the size of the tile.
    pub fn size(self, size: (u16, u16)) -> TileBuilder {
        let mut s = self;
        s.size = Some(size);
        s
    }

    /// Sets the sender of the tile.
    pub fn sender(self, sender: Sender<Msg>) -> TileBuilder {
        let mut s = self;
        s.sender = Some(sender);
        s
    }

    /// Builds the tile.
    pub fn build(self) -> Option<Tile> {
        let (x, y) = self.position?;
        let (w, h) = self.size?;

        Some(Tile {
            command: self.command?,
            coords: self.coords?,
            outer_position: (x, y),
            inner_position: (x + 2, y + 3),
            outer_size: (w, h),
            inner_size: (w - 4, h - 5),
            sender: self.sender?,
            stdout: vec![String::new()],
            scroll: 0,
            counting: true,
            column_number: 0,
            child: None,
        })
    }
}

/// A tile with a command running inside it.
pub struct Tile {
    /// The command that should be executed in the tile.
    pub command: Vec<String>,

    /// Content of the command's stdout and stderr.
    ///
    /// We put both stdout and stderr here to avoid dealing with order between stdout and stderr.
    pub stdout: Vec<String>,

    /// The sender for the communication with the multiview.
    pub sender: Sender<Msg>,

    /// Coordinates of the tile.
    pub coords: (u16, u16),

    /// Top left corner of the tile.
    pub outer_position: (u16, u16),

    /// Top left corner of the content of the tile.
    pub inner_position: (u16, u16),

    /// Size of the tile.
    pub outer_size: (u16, u16),

    /// Size of the inside of the tile.
    pub inner_size: (u16, u16),

    /// The number of lines that the stdout is scrolled.
    pub scroll: isize,

    /// Whether the characters arriving on stdout will move the cursor or not.
    ///
    /// Commands changing the text style won't move the cursor.
    pub counting: bool,

    /// The number of the current column.
    pub column_number: u16,

    /// The PTY and the child process of the command running in the tile.
    pub child: Option<(Pty, Child)>,
}

impl Tile {
    /// Starts the commands.
    pub fn start(&mut self) {
        let command = self
            .command
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();

        let coords = self.coords;
        let clone = command.clone();
        let size = self.inner_size;
        let sender = self.sender.clone();

        let pty = Pty::new().unwrap();
        pty.resize(pty_process::Size::new(size.1, size.0)).unwrap();

        let mut child = Command::new(&clone[0])
            .args(&clone[1..])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn(&pty.pts().unwrap())
            .unwrap();

        let mut stdout = child.stdout.take().unwrap();
        let mut stderr = child.stderr.take().unwrap();
        let stderr_sender = sender.clone();

        let coords = coords;

        thread::spawn(move || {
            loop {
                let mut buffer = [0; 4096];
                let result = stdout.read(&mut buffer);

                match result {
                    Ok(0) => break,

                    Ok(n) => {
                        sender
                            .send(Msg::Stderr(
                                coords,
                                String::from_utf8_lossy(&buffer[0..n]).to_string(),
                            ))
                            .unwrap();
                    }

                    Err(_) => break,
                }
            }

            sender
                .send(Msg::Stdout(coords, String::from("\n")))
                .unwrap();

            let code = 0;

            let exit_string = format!(
                "{}{}Command exited with return code {}\r{}",
                style::Bold,
                if code == 0 {
                    color::Green.fg_str()
                } else {
                    color::Red.fg_str()
                },
                code,
                style::Reset,
            );

            sender.send(Msg::Stdout(coords, exit_string)).unwrap();

            let mut line = String::new();
            for _ in 0..size.0 - 1 {
                line.push('─');
            }

            sender
                .send(Msg::Stdout(coords, format!("\n{}\n", line)))
                .unwrap();
        });

        thread::spawn(move || loop {
            let mut buffer = [0; 4096];
            let result = stderr.read(&mut buffer);

            match result {
                Ok(0) => break,

                Ok(n) => {
                    stderr_sender
                        .send(Msg::Stderr(
                            coords,
                            String::from_utf8_lossy(&buffer[0..n]).to_string(),
                        ))
                        .unwrap();
                }

                Err(_) => break,
            }
        });

        self.child = Some((pty, child));
    }

    /// Push content into the stdout of the tile.
    pub fn push_stdout(&mut self, content: String) {
        for c in content.chars() {
            if c == '\x1b' {
                self.counting = false;
            }

            match c {
                '\n' => {
                    self.stdout.last_mut().unwrap().push(c);
                    self.stdout.push(String::new());
                    self.column_number = 0;
                }

                '\r' => {
                    self.stdout.last_mut().unwrap().push(c);
                    self.column_number = 0;
                }

                _ => {
                    self.stdout.last_mut().unwrap().push(c);

                    // Emoji variation selectors have no length
                    let is_variation_selector = c >= '\u{fe00}' && c <= '\u{fe0f}';

                    if self.counting && !is_variation_selector {
                        self.column_number += 1;
                        if self.column_number == self.inner_size.0 {
                            self.stdout.push(String::new());
                            self.column_number = 0;
                        }
                    }
                }
            }

            if c == 'm' || c == 'K' {
                self.counting = true;
            }
        }

        // Autoscroll whene content arrives on stdout
        self.scroll = self.stdout.len() as isize - 1 - (self.inner_size.1 as isize);
        if self.scroll < 0 {
            self.scroll = 0;
        }
    }

    /// Renders the borders of the tile.
    pub fn render_border(&self, selected: bool) -> String {
        let (x, y) = self.outer_position;
        let (w, h) = self.outer_size;

        let command_str = self.command.join(" ");

        let mut buffer = vec![];

        let max_title_len = self.inner_size.0 - "Command: ".len() as u16;

        let command_str = if command_str.len() > max_title_len as usize {
            format!(
                "{}...",
                &command_str[0 as usize..max_title_len as usize - 3]
            )
        } else {
            command_str
        };

        buffer.push(format!(
            "{}{} {}Command: {}{}{}",
            color::Reset.fg_str(),
            cursor::Goto(x + 1, y + 1),
            style::Bold,
            command_str,
            style::Reset,
            cursor::Goto(x + 2, y + 3),
        ));

        if selected {
            buffer.push(format!("{}", color::Green.fg_str()));
        }

        buffer.push(utils::rect((x, y), (x + w - 1, y + h - 1)));
        buffer.push(format!("{}├", cursor::Goto(x, y + 2)));

        for _ in (x + 1)..(x + w) {
            buffer.push(format!("─"));
        }

        buffer.push(format!(
            "{}┤{}",
            cursor::Goto(x + w - 1, y + 2),
            style::Reset,
        ));

        buffer.join("")
    }

    /// Renders the content of the tile.
    pub fn render_content(&self, selected: bool) -> String {
        const DELETE_CHAR: char = ' ';

        let (x, y) = self.inner_position;
        let (w, h) = self.inner_size;

        let mut buffer = vec![];

        let mut current_char_index = 0;
        let mut max_char_index = 0;

        let scroll = self.scroll as u16;
        let mut line_index = scroll;
        let mut last_line_index = line_index;

        buffer.push(format!("{}", cursor::Goto(x, y)));

        let mut iter = self
            .stdout
            .iter()
            .skip(scroll as usize)
            .take(h as usize + 1);

        let mut line = iter.next().unwrap();
        let mut char_iter = line.chars();

        loop {
            let c = match char_iter.next() {
                Some(c) => c,
                None => match iter.next() {
                    Some(l) => {
                        line = l;
                        char_iter = line.chars();
                        continue;
                    }
                    None => break,
                },
            };

            if c == '\x1b' {
                let mut subbuffer = vec![c];

                loop {
                    let next = match char_iter.next() {
                        Some(c) => c,
                        None => {
                            match iter.next() {
                                Some(l) => {
                                    line = l;
                                    char_iter = line.chars();
                                    continue;
                                }
                                None => break,
                            };
                        }
                    };

                    subbuffer.push(next);

                    if next == 'm' || next == 'K' {
                        break;
                    }
                }

                match (subbuffer.get(0), subbuffer.get(1), subbuffer.get(2)) {
                    (Some('\x1b'), Some('['), Some('K')) => {
                        if current_char_index < w {
                            let mut spaces = String::new();
                            for _ in current_char_index..w {
                                spaces.push(DELETE_CHAR);
                            }
                            buffer.push(format!(
                                "{}{}{}",
                                cursor::Goto(
                                    x + current_char_index,
                                    y + line_index as u16 - scroll
                                ),
                                spaces,
                                cursor::Goto(
                                    x + current_char_index,
                                    y + line_index as u16 - scroll
                                ),
                            ));
                        }
                    }
                    _ => buffer.push(subbuffer.into_iter().collect()),
                }

                continue;
            }

            match c {
                '\n' => {
                    let mut spaces = format!(
                        "{}",
                        cursor::Goto(x + max_char_index, y + line_index as u16 - scroll)
                    );
                    for _ in max_char_index..w {
                        spaces.push(DELETE_CHAR);
                    }
                    buffer.push(spaces);

                    line_index += 1;
                    current_char_index = 0;
                    max_char_index = 0;

                    buffer.push(format!(
                        "{}",
                        cursor::Goto(x, y + line_index as u16 - scroll)
                    ));

                    last_line_index = line_index;
                }

                '\r' => {
                    current_char_index = 0;
                    buffer.push(format!(
                        "{}",
                        cursor::Goto(x, y + line_index as u16 - scroll)
                    ));

                    last_line_index = line_index;
                }

                _ => {
                    // Emoji variation selectors have no length
                    let is_variation_selector = c >= '\u{fe00}' && c <= '\u{fe0f}';

                    if !is_variation_selector {
                        current_char_index += UnicodeWidthChar::width(c).unwrap_or(0) as u16;
                        max_char_index = std::cmp::max(max_char_index, current_char_index);
                    }

                    if current_char_index == w + 1 {
                        line_index += 1;
                        current_char_index = 1;
                        max_char_index = 1;

                        buffer.push(format!(
                            "{}",
                            cursor::Goto(x, y + line_index as u16 - scroll)
                        ));

                        last_line_index = line_index;
                    }

                    buffer.push(format!("{}", c));
                }
            }
        }

        if last_line_index as u16 - scroll <= h {
            let mut spaces = format!(
                "{}",
                cursor::Goto(x + max_char_index, y + last_line_index as u16 - scroll)
            );

            for _ in max_char_index..w {
                spaces.push(DELETE_CHAR);
            }
            buffer.push(spaces);
        }

        // Render scrollbar,thanks @gdamms
        // I have no idea what this code does, I copied/pasted it from gdamms, and then modified
        // some stuff so that it would look right
        if last_line_index > h {
            let mut subbuffer = vec![];
            subbuffer.push(format!(
                "{}{}{}{}",
                style::Reset,
                if selected { color::Green.fg_str() } else { "" },
                cursor::Goto(x + w + 1, y),
                "▲"
            ));

            let bar_portion = h as f32 / self.stdout.len() as f32;
            let bar_nb = f32::max(1.0, (bar_portion * (h) as f32).round()) as u16;
            let max_scroll = self.stdout.len() as isize - h as isize - 1;

            let (scroll_nb_bottom, scroll_nb_top) = if self.scroll > max_scroll / 2 {
                let scroll_nb_bottom = (self.stdout.len() as isize - self.scroll) as u16 - h;
                let scroll_nb_bottom = scroll_nb_bottom as f32 / self.stdout.len() as f32;
                let scroll_nb_bottom = (scroll_nb_bottom * (h as f32)).ceil() as u16;
                let scroll_nb_top = h - bar_nb - scroll_nb_bottom;
                (scroll_nb_bottom, scroll_nb_top)
            } else {
                let scroll_nb_top = self.scroll as f32 / self.stdout.len() as f32;
                let scroll_nb_top = (scroll_nb_top * (h) as f32).ceil() as u16;
                let scroll_nb_bottom = h - bar_nb - scroll_nb_top;
                (scroll_nb_bottom, scroll_nb_top)
            };

            for i in 1..=scroll_nb_top {
                subbuffer.push(format!("{}{}", cursor::Goto(x + w + 1, y + i), "│"));
            }
            for i in scroll_nb_top + 1..=scroll_nb_top + bar_nb {
                subbuffer.push(format!("{}{}", cursor::Goto(x + w + 1, y + i), "█"));
            }
            for i in scroll_nb_top + bar_nb + 1..=scroll_nb_top + bar_nb + scroll_nb_bottom {
                subbuffer.push(format!("{}{}", cursor::Goto(x + w + 1, y + i), "│"));
            }

            subbuffer.push(format!("{}{}", cursor::Goto(x + w + 1, y + h), "▼"));

            buffer.push(subbuffer.join(""));
        }

        buffer.push(format!("{}", style::Reset));
        buffer.join("")
    }

    /// Scrolls up one line.
    pub fn scroll_up(&mut self) {
        if self.scroll > 0 {
            self.scroll -= 1;
        }
    }

    /// Scrolls down one line.
    pub fn scroll_down(&mut self) {
        if self.scroll + (self.inner_size.1 as isize) < self.stdout.len() as isize - 1 {
            self.scroll += 1;
        }
    }

    /// Scrolls up one line.
    pub fn scroll_full_up(&mut self) {
        self.scroll = 0;
    }

    /// Scrolls down one line.
    pub fn scroll_full_down(&mut self) {
        self.scroll = self.stdout.len() as isize - self.inner_size.1 as isize - 1;
        if self.scroll < 0 {
            self.scroll = 0;
        }
    }

    /// Kill the child command.
    pub fn kill(&mut self) -> io::Result<()> {
        if let Some((_, child)) = self.child.as_mut() {
            child.kill()?;
        }

        Ok(())
    }

    /// Restarts the child command.
    pub fn restart(&mut self) -> io::Result<()> {
        self.kill()?;
        self.start();
        Ok(())
    }
}
