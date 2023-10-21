use std::io::{self, stdin, stdout, Read, Write};
use std::process::Stdio;
use std::sync::mpsc::{channel, Sender};
use std::{env, thread};

use pty_process::blocking::Command;
use pty_process::blocking::Pty;

use termion::event::{Event, Key, MouseEvent};
use termion::input::{MouseTerminal, TermRead};
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;
use termion::terminal_size;
use termion::{clear, color, cursor, style};

/// Returns the length of a string containing colors and styles.
pub fn str_len(s: &str) -> u16 {
    let mut count = 0;
    let mut counting = true;
    let mut iter = s.chars().peekable();

    loop {
        let current = match iter.next() {
            Some(c) => c,
            None => break,
        };

        let next = iter.peek();

        if current == '\x1b' && next == Some(&'[') {
            counting = false;
        }

        if counting {
            count += 1;
        }

        if current == 'm' {
            counting = true;
        }
    }

    count
}

/// Returns a substring of a string containing colors and styles.
pub fn sub_str<'a>(s: &'a str, start: u16, end: u16) -> &'a str {
    let mut counting = true;
    let mut iter = s.chars().peekable();

    // Find the start
    let mut real_start = 0;
    let mut logical_start = 0;
    loop {
        if logical_start == start {
            break;
        }

        let current = match iter.next() {
            Some(c) => c,
            None => break,
        };

        let next = iter.peek();

        if current == '\x1b' && next == Some(&'[') {
            counting = false;
        }

        real_start += 1;
        if counting {
            logical_start += 1;
        }

        if current == 'm' {
            counting = true;
        }
    }

    // Find the end
    let mut real_end = real_start;
    let mut logical_end = logical_start;
    loop {
        if logical_end == end {
            break;
        }

        let current = match iter.next() {
            Some(c) => c,
            None => break,
        };

        let next = iter.peek();

        if current == '\x1b' && next == Some(&'[') {
            counting = false;
        }

        if counting {
            logical_end += 1;
        }
        real_end += 1;

        if current == 'm' {
            counting = true;
        }
    }

    &s[real_start..real_end]
}

#[cfg(test)]
mod test {
    use termion::color;

    use crate::str_len;

    #[test]
    fn test_str_len_1() {
        let string = format!(
            "{}Hello{} {}World{}",
            color::Red.fg_str(),
            color::Reset.fg_str(),
            color::Green.fg_str(),
            color::Reset.fg_str(),
        );

        assert_eq!(str_len(&string), 11);
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
    pub stdout: Vec<String>,

    /// The sender for the communication with the multiview.
    pub sender: Sender<Msg>,

    /// Coordinates of the tile.
    pub coords: (u16, u16),

    /// The number of lines that the stdout is scrolled.
    pub scroll: isize,

    /// The number of lines that stdout will print.
    pub max_scroll: isize,
}

impl Tile {
    /// Creates a new empty tile.
    pub fn new(command: &[String], i: u16, j: u16, sender: Sender<Msg>) -> Tile {
        Tile {
            command: command
                .into_iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>(),
            stdout: vec![],
            sender,
            coords: (i, j),
            scroll: 0,
            max_scroll: 0,
        }
    }

    /// Starts the commands.
    pub fn start(&mut self, width: u16, height: u16) {
        let command = self
            .command
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();

        let coords = self.coords;
        let clone = command.clone();
        let sender = self.sender.clone();

        thread::spawn(move || {
            let pty = Pty::new().unwrap();
            pty.resize(pty_process::Size::new(height - 4, width - 4))
                .unwrap();

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
                let mut buffer = [0; 256];
                let result = stderr.read(&mut buffer);

                match result {
                    Ok(0) => break,

                    Ok(n) => {
                        stderr_sender
                            .send(Msg::Stderr(
                                coords.0,
                                coords.1,
                                String::from_utf8_lossy(&buffer[0..n]).to_string(),
                            ))
                            .unwrap();
                    }

                    Err(_) => break,
                }
            });

            loop {
                let mut buffer = [0; 256];
                let result = stdout.read(&mut buffer);

                match result {
                    Ok(0) => break,

                    Ok(n) => {
                        sender
                            .send(Msg::Stderr(
                                coords.0,
                                coords.1,
                                String::from_utf8_lossy(&buffer[0..n]).to_string(),
                            ))
                            .unwrap();
                    }

                    Err(_) => break,
                }
            }

            sender
                .send(Msg::Stdout(coords.0, coords.1, String::from("\n")))
                .unwrap();

            let code = child.wait().unwrap().code().unwrap();

            let exit_string = format!(
                "{}{}Command exited with return code {}{}{}",
                style::Bold,
                if code == 0 {
                    color::Green.fg_str()
                } else {
                    color::Red.fg_str()
                },
                code,
                style::Reset,
                color::Reset.fg_str()
            );

            sender
                .send(Msg::Stdout(coords.0, coords.1, exit_string))
                .unwrap();
        });
    }
}

/// Multiple applications running in a single terminal.
struct Multiview<W: Write> {
    /// The stdout on which the multiview will be rendererd.
    pub stdout: W,

    /// The tiles of the multiview.
    pub tiles: Vec<Vec<Tile>>,

    /// The coordinates of the selected tiles.
    pub selected: (u16, u16),
}

impl<W: Write> Multiview<W> {
    /// Creates a new multiview.
    pub fn new(stdout: W, tiles: Vec<Vec<Tile>>) -> io::Result<Multiview<W>> {
        let mut multiview = Multiview {
            stdout,
            tiles,
            selected: (0, 0),
        };

        write!(
            multiview.stdout,
            "{}{}{}┌",
            clear::All,
            cursor::Hide,
            cursor::Goto(1, 1)
        )?;

        multiview.stdout.flush()?;

        Ok(multiview)
    }

    /// Helper to easily access a tile.
    pub fn tile(&self, (i, j): (u16, u16)) -> &Tile {
        &self.tiles[i as usize][j as usize]
    }

    /// Helper to easily access a mut tile.
    pub fn tile_mut(&mut self, (i, j): (u16, u16)) -> &mut Tile {
        &mut self.tiles[i as usize][j as usize]
    }

    /// Sets the selected tile from (x, y) coordinates.
    pub fn select_tile(&mut self, (x, y): (u16, u16), term_size: (u16, u16)) {
        let w = term_size.0 / self.tiles[0].len() as u16;
        let h = term_size.1 / self.tiles.len() as u16;

        self.selected = (y / h, x / w);
    }

    /// Draws a box from (x1, y1) to (x2, y2).
    pub fn rect(&mut self, (x1, y1): (u16, u16), (x2, y2): (u16, u16)) -> String {
        let mut buffer = vec![];

        buffer.push(format!("{}┌", cursor::Goto(x1, y1)));

        for _ in (x1 + 1)..x2 {
            buffer.push(format!("─"));
        }

        buffer.push(format!("┐"));

        for y in (y1 + 1)..y2 {
            buffer.push(format!("{}│", cursor::Goto(x1, y)));
            buffer.push(format!("{}│", cursor::Goto(x2, y)));
        }

        buffer.push(format!("{}└", cursor::Goto(x1, y2)));

        for _ in (x1 + 1)..x2 {
            buffer.push(format!("─"));
        }

        buffer.push(format!("┘"));

        buffer.join("")
    }

    /// Renders the (x, y) tile.
    pub fn render_tile(&mut self, (i, j): (u16, u16), term_size: (u16, u16)) -> String {
        let w = term_size.0 / self.tiles[0].len() as u16;
        let h = term_size.1 / self.tiles.len() as u16;

        let x1 = j * w + 1;
        let y1 = i * h + 1;

        let x2 = (j + 1) * w;
        let y2 = (i + 1) * h;

        let tile = &self.tile((i, j));
        let command_str = tile.command.join(" ");

        let mut buffer = vec![];

        // TODO: find a way to avoid this copy
        let lines = tile
            .stdout
            .iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();

        let max_title_len = term_size.0 - 4 - "Command: ".len() as u16;

        let command_str = if str_len(&command_str) > max_title_len {
            format!("{}...", sub_str(&command_str, 0, max_title_len - 3))
        } else {
            command_str
        };

        let mut counting = true;
        let mut line_index = 0;
        let mut current_char_index = 0;
        let scroll = tile.scroll as u16;

        buffer.push(format!(
            "{}{} {}Command: {}{}{}",
            color::Reset.fg_str(),
            cursor::Goto(x1 + 1, y1 + 1),
            style::Bold,
            command_str,
            style::Reset,
            cursor::Goto(x1 + 2, y1 + 3),
        ));

        for line in lines {
            for c in line.chars() {
                if c == '\x1b' {
                    counting = false;
                }

                match c {
                    '\n' => {
                        line_index += 1;
                        current_char_index = 0;

                        if line_index >= scroll && line_index < h + scroll {
                            if current_char_index < w {
                                let mut spaces = String::new();
                                for _ in 0..w - current_char_index {
                                    spaces.push(' ');
                                }
                                buffer.push(spaces);
                            }

                            buffer.push(format!(
                                "{}",
                                cursor::Goto(x1 + 2, y1 + 3 + line_index as u16 - scroll)
                            ));
                        }
                    }

                    '\r' => {
                        current_char_index = 0;

                        if line_index >= scroll && line_index < h + scroll {
                            buffer.push(format!(
                                "{}",
                                cursor::Goto(x1 + 2, y1 + 3 + line_index as u16 - scroll)
                            ));
                        }
                    }

                    _ => {
                        if counting {
                            current_char_index += 1;
                        }

                        if current_char_index == w - 3 {
                            line_index += 1;
                            current_char_index = 1;

                            if line_index >= scroll && line_index < h + scroll {
                                buffer.push(format!(
                                    "{}",
                                    cursor::Goto(x1 + 2, y1 + 3 + line_index as u16 - scroll)
                                ));
                            }
                        }

                        if line_index >= scroll && line_index < h + scroll {
                            buffer.push(format!("{}", c));
                        }
                    }
                }

                if c == 'm' {
                    counting = true;
                }
            }
        }

        let tile = self.tile_mut((i, j));
        if tile.max_scroll != line_index as isize {
            tile.max_scroll = line_index as isize;
            tile.scroll = tile.max_scroll - h as isize + 5;
            if tile.scroll < 0 {
                tile.scroll = 0;
            }
        }

        if self.selected == (i, j) {
            buffer.push(format!("{}", color::Green.fg_str()));
        }
        buffer.push(self.rect((x1, y1), (x2, y2)));
        buffer.push(format!("{}├", cursor::Goto(x1, y1 + 2)));

        for _ in (x1 + 1)..x2 {
            buffer.push(format!("─"));
        }

        buffer.push(format!("{}┤", cursor::Goto(x2, y1 + 2)));

        buffer.join("")
    }

    /// Renders all the tiles of the multiview.
    pub fn render(&mut self, term_size: (u16, u16)) -> io::Result<()> {
        let mut buffer = vec![];
        for i in 0..self.tiles.len() {
            for j in 0..self.tiles[0].len() {
                buffer.push(self.render_tile((i as u16, j as u16), term_size));
            }
        }

        write!(self.stdout, "{}", buffer.join(""))?;
        self.stdout.flush()?;

        Ok(())
    }

    /// Scrolls down the current selected tile.
    pub fn scroll_down(&mut self) {
        self.tile_mut(self.selected).scroll += 1;
    }

    /// Scrolls up the current selected tile.
    pub fn scroll_up(&mut self) {
        let tile = self.tile_mut(self.selected);
        if tile.scroll > 0 {
            tile.scroll -= 1;
        }
    }
}

impl<W: Write> Drop for Multiview<W> {
    fn drop(&mut self) {
        write!(self.stdout, "{}", cursor::Show).unwrap();
    }
}

/// An event that can be sent in channels.
pub enum Msg {
    /// An stdout line arrived.
    Stdout(u16, u16, String),

    /// An stderr line arrived.
    Stderr(u16, u16, String),

    /// A click occured.
    Click(u16, u16),

    /// Scroll up one line.
    ScrollUp,

    /// Scroll down one line.
    ScrollDown,

    /// The program was asked to exit.
    Exit,
}

/// Starts the multiview application.
pub fn main() -> io::Result<()> {
    let (sender, receiver) = channel();

    let args = env::args().skip(1).collect::<Vec<_>>();

    let tiles = args
        .split(|x| x == "//")
        .map(|x| x.split(|y| y == "::").enumerate().collect::<Vec<_>>())
        .enumerate()
        .map(|(i, tiles)| {
            tiles
                .into_iter()
                .map(|(j, tile)| Tile::new(tile, i as u16, j as u16, sender.clone()))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let stdin = stdin();
    let stdout = stdout().into_raw_mode()?;
    let stdout = stdout.into_alternate_screen()?;
    let stdout = MouseTerminal::from(stdout);

    let term_size = terminal_size()?;

    let mut multiview = Multiview::new(stdout, tiles)?;
    multiview.render(term_size)?;

    let tile_size = (
        term_size.0 / multiview.tiles[0].len() as u16,
        term_size.1 / multiview.tiles.len() as u16,
    );

    for row in &mut multiview.tiles {
        for tile in row {
            tile.start(tile_size.0, tile_size.1);
        }
    }

    thread::spawn(move || {
        for c in stdin.events() {
            let evt = c.unwrap();
            match evt {
                Event::Key(Key::Char('q')) => sender.send(Msg::Exit).unwrap(),
                Event::Key(Key::Down) => sender.send(Msg::ScrollDown).unwrap(),
                Event::Key(Key::Up) => sender.send(Msg::ScrollUp).unwrap(),

                Event::Mouse(me) => match me {
                    MouseEvent::Press(_, x, y) => sender.send(Msg::Click(x, y)).unwrap(),
                    _ => (),
                },

                _ => {}
            }
        }
    });

    loop {
        match receiver.recv() {
            Ok(Msg::Stdout(i, j, line)) => multiview.tile_mut((i, j)).stdout.push(line),
            Ok(Msg::Stderr(i, j, line)) => multiview.tile_mut((i, j)).stdout.push(line),
            Ok(Msg::Click(x, y)) => multiview.select_tile((x, y), term_size),
            Ok(Msg::ScrollDown) => multiview.scroll_down(),
            Ok(Msg::ScrollUp) => multiview.scroll_up(),
            Ok(Msg::Exit) => break,
            Err(_) => (),
        }

        multiview.render(term_size)?;
    }

    Ok(())
}
