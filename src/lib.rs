use std::io::{self, stdin, stdout, BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc::{channel, Sender};
use std::{env, thread};

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
}

impl Tile {
    /// Creates a new empty tile.
    pub fn new(command: &[String], i: u16, j: u16, sender: Sender<Msg>) -> Tile {
        let command = command
            .into_iter()
            .map(|x| x.to_string())
            .collect::<Vec<_>>();

        let clone = command.clone();

        thread::spawn(move || {
            let mut child = Command::new(&clone[0])
                .args(&clone[1..])
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap();

            let stdout = child.stdout.take().unwrap();
            let reader = BufReader::new(stdout);

            let mut lines = reader.lines();

            loop {
                match lines.next() {
                    Some(Ok(line)) => {
                        sender.send(Msg::Stdout(i, j, line)).unwrap();
                    }

                    Some(Err(_)) => {
                        break;
                    }

                    None => break,
                }
            }

            sender.send(Msg::Stdout(i, j, String::new())).unwrap();

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

            sender.send(Msg::Stdout(i, j, exit_string)).unwrap();
        });

        Tile {
            command: command
                .into_iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>(),
            stdout: vec![],
        }
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
    pub fn rect(&mut self, (x1, y1): (u16, u16), (x2, y2): (u16, u16)) -> io::Result<()> {
        write!(self.stdout, "{}┌", cursor::Goto(x1, y1))?;

        for _ in (x1 + 1)..x2 {
            write!(self.stdout, "─")?;
        }

        write!(self.stdout, "┐")?;

        for y in (y1 + 1)..y2 {
            write!(self.stdout, "{}│", cursor::Goto(x1, y))?;
            write!(self.stdout, "{}│", cursor::Goto(x2, y))?;
        }

        write!(self.stdout, "{}└", cursor::Goto(x1, y2))?;

        for _ in (x1 + 1)..x2 {
            write!(self.stdout, "─")?;
        }

        write!(self.stdout, "┘")?;

        Ok(())
    }

    /// Clears stdout.
    pub fn clear(&mut self) -> io::Result<()> {
        write!(self.stdout, "{}", clear::All)
    }

    /// Renders the (x, y) tile.
    pub fn render_tile(&mut self, (i, j): (u16, u16), term_size: (u16, u16)) -> io::Result<()> {
        let w = term_size.0 / self.tiles[0].len() as u16;
        let h = term_size.1 / self.tiles.len() as u16;

        let x1 = j * w + 1;
        let y1 = i * h + 1;

        let x2 = (j + 1) * w;
        let y2 = (i + 1) * h;

        let tile = &self.tile((i, j));
        let command_str = tile.command.join(" ");

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

        write!(
            self.stdout,
            "{}{} {}Command: {}{}",
            color::Reset.fg_str(),
            cursor::Goto(x1 + 1, y1 + 1),
            style::Bold,
            command_str,
            style::Reset,
        )?;

        let mut line_index = 0;
        for line in lines {
            let mut len = str_len(&line) as i32;
            let mut current_char_index = 0;

            if len == 0 {
                line_index += 1;
                continue;
            }

            while len > 0 {
                let sub = sub_str(
                    &line,
                    current_char_index,
                    current_char_index + term_size.0 - 4,
                );
                write!(
                    self.stdout,
                    "{}{}",
                    cursor::Goto(x1 + 2, y1 + 3 + line_index as u16),
                    sub.replace(
                        "\r",
                        &format!("{}", cursor::Goto(x1 + 2, y1 + 3 + line_index as u16))
                    ),
                )?;

                // if sub.contains(|x| x == '\r') {
                //     line_index -= 1;
                // }

                line_index += 1;
                len -= (term_size.0 - 4) as i32;
                current_char_index += term_size.0 - 4;
            }
        }

        if self.selected == (i, j) {
            write!(self.stdout, "{}", color::Green.fg_str())?;
        }
        self.rect((x1, y1), (x2, y2))?;
        write!(self.stdout, "{}├", cursor::Goto(x1, y1 + 2))?;

        for _ in (x1 + 1)..x2 {
            write!(self.stdout, "─")?;
        }

        write!(self.stdout, "{}┤", cursor::Goto(x2, y1 + 2))?;

        Ok(())
    }

    /// Renders all the tiles of the multiview.
    pub fn render(&mut self, term_size: (u16, u16)) -> io::Result<()> {
        self.clear()?;

        for i in 0..self.tiles.len() {
            for j in 0..self.tiles[0].len() {
                self.render_tile((i as u16, j as u16), term_size)?;
            }
        }

        self.stdout.flush()?;

        Ok(())
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

    /// A click occured.
    Click(u16, u16),

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

    thread::spawn(move || {
        for c in stdin.events() {
            let evt = c.unwrap();
            match evt {
                Event::Key(Key::Char('q')) => sender.send(Msg::Exit).unwrap(),

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
            Ok(Msg::Click(x, y)) => multiview.select_tile((x, y), term_size),
            Ok(Msg::Exit) => break,
            Err(_) => (),
        }

        multiview.render(term_size)?;
    }

    Ok(())
}
