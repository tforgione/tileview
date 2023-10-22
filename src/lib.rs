use std::io::{self, stdin, stdout, Write};
use std::sync::mpsc::channel;
use std::{env, thread};

use termion::event::{Event, Key, MouseButton, MouseEvent};
use termion::input::{MouseTerminal, TermRead};
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;
use termion::terminal_size;
use termion::{clear, cursor};

pub mod tile;
pub mod utils;

use tile::{Tile, TileBuilder};

/// Multiple applications running in a single terminal.
struct Multiview<W: Write> {
    /// The stdout on which the multiview will be rendererd.
    pub stdout: W,

    /// The tiles of the multiview.
    pub tiles: Vec<Vec<Tile>>,

    /// The coordinates of the selected tiles.
    pub selected: (u16, u16),

    /// Whether we need to refresh the UI.
    pub refresh_ui: bool,
}

impl<W: Write> Multiview<W> {
    /// Creates a new multiview.
    pub fn new(stdout: W, tiles: Vec<Vec<Tile>>) -> io::Result<Multiview<W>> {
        let mut multiview = Multiview {
            stdout,
            tiles,
            selected: (0, 0),
            refresh_ui: true,
        };

        write!(
            multiview.stdout,
            "{}{}{}",
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
        self.refresh_ui = true;
    }

    /// Renders the border and the title of a tile.
    pub fn render_tile_border(&self, (i, j): (u16, u16)) -> String {
        let tile = &self.tile((i, j));
        tile.render_border(self.selected == ((i, j)))
    }

    /// Renders the (x, y) tile.
    pub fn render_tile_content(&mut self, (i, j): (u16, u16)) -> String {
        let tile = self.tile((i, j));
        tile.render_content()
    }

    /// Renders all the tiles of the multiview.
    pub fn render(&mut self) -> io::Result<()> {
        let mut buffer = vec![];
        for i in 0..self.tiles.len() {
            for j in 0..self.tiles[0].len() {
                if self.refresh_ui {
                    buffer.push(self.render_tile_border((i as u16, j as u16)));
                }
                buffer.push(self.render_tile_content((i as u16, j as u16)));
            }
        }

        self.refresh_ui = false;
        write!(self.stdout, "{}", buffer.join(""))?;
        self.stdout.flush()?;

        Ok(())
    }

    /// Scrolls down the current selected tile.
    pub fn scroll_down(&mut self) {
        let tile = self.tile_mut(self.selected);
        tile.scroll_down();
    }

    /// Scrolls up the current selected tile.
    pub fn scroll_up(&mut self) {
        let tile = self.tile_mut(self.selected);
        tile.scroll_up();
    }

    /// Push a string into a tile's stdout.
    pub fn push_stdout(&mut self, (i, j): (u16, u16), content: String) {
        let tile = self.tile_mut((i, j));
        tile.push_stdout(content);
    }

    /// Push a string into a tile's stderr.
    pub fn push_stderr(&mut self, (i, j): (u16, u16), content: String) {
        self.push_stdout((i, j), content);
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
    Stdout((u16, u16), String),

    /// An stderr line arrived.
    Stderr((u16, u16), String),

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
                .map(|(j, tile)| ((i, j), tile))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let term_size = terminal_size()?;

    let tile_size = (
        term_size.0 / tiles[0].len() as u16,
        term_size.1 / tiles.len() as u16,
    );

    let tiles = tiles
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|((i, j), tile)| {
                    TileBuilder::new()
                        .command(tile.into())
                        .coords((i as u16, j as u16))
                        .position((j as u16 * tile_size.0 + 1, i as u16 * tile_size.1 + 1))
                        .size(tile_size)
                        .sender(sender.clone())
                        .build()
                        .unwrap()
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let stdin = stdin();
    let stdout = stdout().into_raw_mode()?;
    let stdout = stdout.into_alternate_screen()?;
    let stdout = MouseTerminal::from(stdout);

    let mut multiview = Multiview::new(stdout, tiles)?;
    multiview.render()?;

    for row in &mut multiview.tiles {
        for tile in row {
            tile.start();
        }
    }

    thread::spawn(move || {
        for c in stdin.events() {
            let evt = c.unwrap();
            match evt {
                Event::Key(Key::Char('q')) => sender.send(Msg::Exit).unwrap(),
                Event::Key(Key::Down) => sender.send(Msg::ScrollDown).unwrap(),
                Event::Key(Key::Up) => sender.send(Msg::ScrollUp).unwrap(),
                Event::Mouse(MouseEvent::Press(p, x, y)) => match p {
                    MouseButton::WheelUp => sender.send(Msg::ScrollUp).unwrap(),
                    MouseButton::WheelDown => sender.send(Msg::ScrollDown).unwrap(),
                    MouseButton::Left => sender.send(Msg::Click(x, y)).unwrap(),
                    _ => (),
                },

                _ => {}
            }
        }
    });

    loop {
        match receiver.recv() {
            Ok(Msg::Stdout(coords, line)) => multiview.push_stdout(coords, line),
            Ok(Msg::Stderr(coords, line)) => multiview.push_stderr(coords, line),
            Ok(Msg::Click(x, y)) => multiview.select_tile((x, y), term_size),
            Ok(Msg::ScrollDown) => multiview.scroll_down(),
            Ok(Msg::ScrollUp) => multiview.scroll_up(),
            Ok(Msg::Exit) => break,
            Err(_) => (),
        }

        multiview.render()?;
    }

    Ok(())
}
