use std::env;
use std::io::{self, stdin, stdout, Write};

use termion::event::{Event, Key, MouseEvent};
use termion::input::{MouseTerminal, TermRead};
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;
use termion::terminal_size;
use termion::{clear, color, cursor, style};

/// A tile with a command running inside it.
#[derive(Debug)]
pub struct Tile {
    /// The command that should be executed in the tile.
    pub command: Vec<String>,
}

impl Tile {
    /// Creates a new empty tile.
    pub fn new(command: &[String]) -> Tile {
        Tile {
            command: command
                .into_iter()
                .map(|x| x.to_string())
                .collect::<Vec<_>>(),
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

        if self.selected == (i, j) {
            write!(self.stdout, "{}", color::Green.fg_str())?;
        }
        self.rect((x1, y1), (x2, y2))?;
        write!(self.stdout, "{}├", cursor::Goto(x1, y1 + 2))?;

        for _ in (x1 + 1)..x2 {
            write!(self.stdout, "─")?;
        }

        write!(self.stdout, "{}┤", cursor::Goto(x2, y1 + 2))?;

        let tile = &self.tile((i, j));

        write!(
            self.stdout,
            "{}{} {}Command: {}{}",
            color::Reset.fg_str(),
            cursor::Goto(x1 + 1, y1 + 1),
            style::Bold,
            tile.command.join(" "),
            style::Reset,
        )?;

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

/// Starts the multiview application.
pub fn main() -> io::Result<()> {
    let args = env::args().skip(1).collect::<Vec<_>>();

    let tiles = args
        .split(|x| x == "//")
        .map(|x| x.split(|y| y == "::").map(Tile::new))
        .map(|x| x.collect::<Vec<_>>())
        .collect::<Vec<_>>();

    let stdin = stdin();
    let stdout = stdout().into_raw_mode()?;
    let stdout = stdout.into_alternate_screen()?;
    let stdout = MouseTerminal::from(stdout);

    let term_size = terminal_size()?;

    let mut multiview = Multiview::new(stdout, tiles)?;
    multiview.render(term_size)?;

    for c in stdin.events() {
        let evt = c?;
        match evt {
            Event::Key(Key::Char('q')) => break,

            Event::Mouse(me) => match me {
                MouseEvent::Press(_, x, y) => multiview.select_tile((x, y), term_size),
                _ => (),
            },

            _ => {}
        }

        multiview.render(term_size)?;
    }

    Ok(())
}
