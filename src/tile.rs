//! This module contains everything related to tiles.

use std::io::Read;
use std::process::Stdio;
use std::sync::mpsc::Sender;
use std::thread;

use pty_process::blocking::Command;
use pty_process::blocking::Pty;

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
            stdout: String::new(),
            cursor: None,
            len: 0,
            scroll: 0,
            number_lines: 0,
        })
    }
}

/// A tile with a command running inside it.
#[derive(Debug)]
pub struct Tile {
    /// The command that should be executed in the tile.
    pub command: Vec<String>,

    /// Content of the command's stdout and stderr.
    ///
    /// We put both stdout and stderr here to avoid dealing with order between stdout and stderr.
    pub stdout: String,

    /// The cursor where stdout should write.
    ///
    /// If None, stdout should push at the end of the string.
    pub cursor: Option<usize>,

    /// The number of chars in stdout.
    pub len: usize,

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

    /// The number of lines that stdout will print.
    pub number_lines: isize,
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

        thread::spawn(move || {
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

            let code = child.wait().unwrap().code().unwrap();

            let exit_string = format!(
                "{}{}Command exited with return code {}{}\n",
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
        });
    }

    /// Push content into the stdout of the tile.
    pub fn push_stdout(&mut self, content: String) {
        let mut clear_line_counter = 0;

        for c in content.chars() {
            // Check if we're running into \x1b[K
            clear_line_counter = match (c, clear_line_counter) {
                ('\x1b', _) => 1,
                ('[', 1) => 2,
                ('K', 2) => 3,
                _ => 0,
            };

            match (clear_line_counter, self.cursor) {
                (3, Some(cursor)) => {
                    // Find the size of the string until the next '\n' or end
                    let mut counter = 0;
                    loop {
                        counter += 1;

                        // TODO fix utf8
                        if self.stdout.len() <= counter + cursor
                            || &self.stdout[cursor + counter..cursor + counter + 1] == "\n"
                        {
                            break;
                        }
                    }

                    self.stdout
                        .replace_range((cursor - 2)..(cursor + counter), "");
                    self.len -= 2 + counter;
                    self.cursor = None;
                    continue;
                }
                _ => (),
            }

            if c == '\r' {
                // Set cursor at the right place
                let mut index = self.len;
                let mut reverse = self.stdout.chars().rev();

                loop {
                    match reverse.next() {
                        Some('\n') | None => break,
                        _ => index -= 1,
                    }
                }

                self.cursor = Some(index);
            } else {
                let new_cursor = match self.cursor {
                    Some(index) => {
                        if c == '\n' {
                            self.stdout.push(c);
                            self.len += 1;
                            None
                        } else {
                            // TODO fix utf8
                            self.stdout.replace_range(index..index + 1, &c.to_string());
                            if index + 1 == self.len {
                                None
                            } else {
                                Some(index + 1)
                            }
                        }
                    }

                    None => {
                        self.stdout.push(c);
                        self.len += 1;
                        None
                    }
                };

                self.cursor = new_cursor;
            }
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
    pub fn render_content(&self) -> String {
        let (x, y) = self.inner_position;
        let (w, h) = self.inner_size;

        let mut buffer = vec![];

        let mut counting = true;
        let mut line_index = 0;
        let mut current_char_index = 0;
        let scroll = self.scroll as u16;

        buffer.push(format!("{}", cursor::Goto(x, y)));

        for c in self.stdout.chars() {
            if c == '\x1b' {
                counting = false;
            }

            match c {
                '\n' => {
                    line_index += 1;
                    let old_current_char_index = current_char_index;
                    current_char_index = 0;

                    if line_index >= scroll && line_index <= h + scroll {
                        if old_current_char_index < w {
                            let mut spaces = String::new();
                            for _ in old_current_char_index..w {
                                spaces.push(' ');
                            }
                            buffer.push(spaces);
                        }

                        buffer.push(format!(
                            "{}",
                            cursor::Goto(x, y + line_index as u16 - scroll)
                        ));
                    }
                }

                _ => {
                    if counting {
                        current_char_index += 1;
                    }

                    if current_char_index == w + 1 {
                        line_index += 1;
                        current_char_index = 1;

                        if line_index >= scroll && line_index <= h + scroll {
                            buffer.push(format!(
                                "{}",
                                cursor::Goto(x, y + line_index as u16 - scroll)
                            ));
                        }
                    }

                    if line_index >= scroll && line_index <= h + scroll {
                        buffer.push(format!("{}", c));
                    }
                }
            }

            if c == 'm' {
                counting = true;
            }
        }

        buffer.push(format!("{}", style::Reset));
        buffer.join("")
    }
}
