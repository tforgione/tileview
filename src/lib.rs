use std::io::{self, stdin, stdout, Write};
use std::sync::mpsc::channel;
use std::time::{Duration, Instant};
use std::{env, thread};

use termion::event::{Event, Key, MouseButton, MouseEvent};
use termion::input::{MouseTerminal, TermRead};
use termion::raw::IntoRawMode;
use termion::screen::IntoAlternateScreen;
use termion::terminal_size;
use termion::{clear, cursor};

use tile::{Tile, TileBuilder};

pub mod tile;
pub mod utils;

const DELAY: Duration = Duration::from_millis(20);

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

    /// Whether we need to refresh the tiles.
    pub refresh_tiles: bool,

    /// Last time when the rendering was performed.
    pub last_render: Instant,
}

impl<W: Write> Multiview<W> {
    /// Creates a new multiview.
    pub fn new(stdout: W, tiles: Vec<Vec<Tile>>) -> io::Result<Multiview<W>> {
        let mut multiview = Multiview {
            stdout,
            tiles,
            selected: (0, 0),
            refresh_ui: true,
            refresh_tiles: false,
            last_render: Instant::now(),
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
    pub fn select_tile(&mut self, (x, y): (u16, u16)) {
        // Ugly but working
        for (i, row) in self.tiles.iter().enumerate() {
            for (j, tile) in row.iter().enumerate() {
                if tile.outer_position.0 <= x && x < tile.outer_position.0 + tile.outer_size.0 {
                    if tile.outer_position.1 <= y && y < tile.outer_position.1 + tile.outer_size.1 {
                        self.selected = (i as u16, j as u16);
                    }
                }
            }
        }
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
        tile.render_content(self.selected == (i, j))
    }

    /// Renders all the tiles of the multiview.
    pub fn render(&mut self, force: bool) -> io::Result<()> {
        if !self.refresh_tiles {
            return Ok(());
        }

        let now = Instant::now();

        if now.duration_since(self.last_render) < DELAY && !force {
            return Ok(());
        }

        self.last_render = now;

        let mut buffer = if self.refresh_ui {
            vec![format!("{}", clear::All)]
        } else {
            vec![]
        };

        for i in 0..self.tiles.len() {
            for j in 0..self.tiles[i].len() {
                if self.refresh_ui {
                    buffer.push(self.render_tile_border((i as u16, j as u16)));
                }
                buffer.push(self.render_tile_content((i as u16, j as u16)));
            }
        }

        self.refresh_ui = false;
        self.refresh_tiles = false;
        write!(self.stdout, "{}", buffer.join(""))?;
        self.stdout.flush()?;

        Ok(())
    }

    /// Scrolls down the current selected tile.
    pub fn scroll_down(&mut self, step: isize) {
        let tile = self.tile_mut(self.selected);
        tile.scroll_down(step);
    }

    /// Scrolls up the current selected tile.
    pub fn scroll_up(&mut self, step: isize) {
        let tile = self.tile_mut(self.selected);
        tile.scroll_up(step);
    }

    /// Scrolls down to the bottom of the current selected tile.
    pub fn scroll_full_down(&mut self) {
        let tile = self.tile_mut(self.selected);
        tile.scroll_full_down();
    }

    /// Scrolls up to the top the current selected tile.
    pub fn scroll_full_up(&mut self) {
        let tile = self.tile_mut(self.selected);
        tile.scroll_full_up();
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

    /// Restarts the selected tile.
    pub fn restart(&mut self) {
        let tile = self.tile_mut(self.selected);
        tile.restart();
    }

    /// Restarts all tiles.
    pub fn restart_all(&mut self) {
        for row in &mut self.tiles {
            for tile in row {
                tile.restart();
            }
        }
    }

    /// Kills the selected tile.
    pub fn kill(&mut self) {
        let tile = self.tile_mut(self.selected);
        tile.kill();
    }

    /// Kills all tiles.
    pub fn kill_all(&mut self) {
        for row in &mut self.tiles {
            for tile in row {
                tile.kill();
            }
        }
    }

    /// Adds a line to the current tile.
    pub fn add_line(&mut self) {
        let tile = self.tile_mut(self.selected);
        tile.add_line();
    }

    /// Adds a line to every tile.
    pub fn add_line_all(&mut self) {
        for row in &mut self.tiles {
            for tile in row {
                tile.add_line();
            }
        }
    }

    /// Adds a finish line to the specified tile.
    pub fn add_finish_line(&mut self, coords: (u16, u16), success: bool) {
        let tile = self.tile_mut(coords);
        tile.add_finish_line(success);
    }

    /// Exits.
    pub fn exit(&mut self) {
        write!(self.stdout, "{}", cursor::Show).ok();

        for row in &mut self.tiles {
            for tile in row {
                tile.kill()
            }
        }
    }

    /// Triggers a click on a certain character.
    pub fn click(&mut self, (i, j): (u16, u16)) {
        self.select_tile((i, j));
        let tile = self.tile_mut(self.selected);
        tile.click((i, j));
    }

    /// Triggers a motion on a certain character.
    pub fn hold(&mut self, (i, j): (u16, u16)) {
        let tile = self.tile_mut(self.selected);
        tile.hold((i, j));
    }

    /// Copies the current selection to the clipboard.
    pub fn copy(&self) {
        let tile = self.tile(self.selected);
        tile.copy();
    }

    /// Treats a message.
    pub fn manage_msg(&mut self, msg: Msg) -> io::Result<()> {
        self.refresh_tiles = true;

        match msg {
            Msg::Stdout(coords, line) => self.push_stdout(coords, line),
            Msg::Stderr(coords, line) => self.push_stderr(coords, line),
            Msg::Click(x, y) => self.click((x, y)),
            Msg::Hold(x, y) => self.hold((x, y)),
            Msg::Restart => self.restart(),
            Msg::RestartAll => self.restart_all(),
            Msg::Kill => self.kill(),
            Msg::KillAll => self.kill_all(),
            Msg::ScrollDown(step) => self.scroll_down(step),
            Msg::ScrollUp(step) => self.scroll_up(step),
            Msg::ScrollFullDown => self.scroll_full_down(),
            Msg::ScrollFullUp => self.scroll_full_up(),
            Msg::AddLine => self.add_line(),
            Msg::AddLineAll => self.add_line_all(),
            Msg::AddFinishLine(coords, success) => self.add_finish_line(coords, success),
            Msg::Copy => self.copy(),
            Msg::Exit => self.exit(),
        }

        Ok(())
    }
}

impl<W: Write> Drop for Multiview<W> {
    fn drop(&mut self) {
        self.exit();
    }
}

/// An event that can be sent in channels.
#[derive(PartialEq, Eq)]
pub enum Msg {
    /// An stdout line arrived.
    Stdout((u16, u16), String),

    /// An stderr line arrived.
    Stderr((u16, u16), String),

    /// A click occured.
    Click(u16, u16),

    /// A holding motion has occured.
    Hold(u16, u16),

    /// Restarts the selected tile.
    Restart,

    /// Restarts all tiles.
    RestartAll,

    /// Kills the selected tile.
    Kill,

    /// Kills all tiles.
    KillAll,

    /// Scroll up one line.
    ScrollUp(isize),

    /// Scroll down one line.
    ScrollDown(isize),

    /// Scroll to the top of the log.
    ScrollFullUp,

    /// Scroll to the bottom of the log.
    ScrollFullDown,

    /// Adds a line to the current tile.
    AddLine,

    /// Adds a line to every tile.
    AddLineAll,

    /// Adds the finish line to the tile.
    AddFinishLine((u16, u16), bool),

    /// Copies the selection to the clipboard.
    Copy,

    /// The program was asked to exit.
    Exit,
}

/// Starts the multiview application.
pub fn main() -> io::Result<()> {
    let (sender, receiver) = channel();

    let args = env::args().skip(1).collect::<Vec<_>>();

    let mut is_row_major = true;

    for arg in &args {
        if arg == "//" {
            is_row_major = false;
            break;
        }

        if arg == "::" {
            is_row_major = true;
            break;
        }
    }

    let (first_split, second_split) = if is_row_major {
        ("//", "::")
    } else {
        ("::", "//")
    };

    let tiles = args
        .split(|x| x == first_split)
        .map(|x| {
            x.split(|y| y == second_split)
                .enumerate()
                .collect::<Vec<_>>()
        })
        .enumerate()
        .map(|(i, tiles)| {
            tiles
                .into_iter()
                .map(|(j, tile)| ((i, j), tile))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let mut term_size = terminal_size()?;

    let col_len = tiles.len() as u16;

    let tiles = tiles
        .into_iter()
        .map(|row| {
            let row_len = row.len() as u16;

            let tile_size = if is_row_major {
                (term_size.0 / row_len, term_size.1 / col_len)
            } else {
                (term_size.0 / col_len, term_size.1 / row_len)
            };

            row.into_iter()
                .map(|((i, j), tile)| {
                    let (p_i, p_j) = if is_row_major { (i, j) } else { (j, i) };

                    TileBuilder::new()
                        .command(tile.into())
                        .coords((i as u16, j as u16))
                        .position((p_j as u16 * tile_size.0 + 1, p_i as u16 * tile_size.1 + 1))
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
    multiview.render(true)?;

    for row in &mut multiview.tiles {
        for tile in row {
            tile.start();
        }
    }

    thread::spawn(move || {
        for c in stdin.events() {
            let evt = c.unwrap();
            match evt {
                Event::Key(Key::Esc) | Event::Key(Key::Ctrl('c')) | Event::Key(Key::Char('q')) => {
                    sender.send(Msg::Exit).unwrap()
                }
                Event::Key(Key::Char('y')) => sender.send(Msg::Copy).unwrap(),
                Event::Key(Key::Char('r')) => sender.send(Msg::Restart).unwrap(),
                Event::Key(Key::Char('R')) => sender.send(Msg::RestartAll).unwrap(),
                Event::Key(Key::Char('k')) => sender.send(Msg::Kill).unwrap(),
                Event::Key(Key::Char('K')) => sender.send(Msg::KillAll).unwrap(),
                Event::Key(Key::Char('l')) => sender.send(Msg::AddLine).unwrap(),
                Event::Key(Key::Char('L')) => sender.send(Msg::AddLineAll).unwrap(),
                Event::Key(Key::Down) => sender.send(Msg::ScrollDown(1)).unwrap(),
                Event::Key(Key::Up) => sender.send(Msg::ScrollUp(1)).unwrap(),
                Event::Key(Key::End) => sender.send(Msg::ScrollFullDown).unwrap(),
                Event::Key(Key::Home) => sender.send(Msg::ScrollFullUp).unwrap(),
                Event::Mouse(MouseEvent::Press(p, x, y)) => match p {
                    MouseButton::WheelUp => sender.send(Msg::ScrollUp(3)).unwrap(),
                    MouseButton::WheelDown => sender.send(Msg::ScrollDown(3)).unwrap(),
                    MouseButton::Left => sender.send(Msg::Click(x, y)).unwrap(),
                    _ => (),
                },
                Event::Mouse(MouseEvent::Hold(x, y)) => sender.send(Msg::Hold(x, y)).unwrap(),

                _ => {}
            }
        }
    });

    loop {
        if let Ok(msg) = receiver.recv_timeout(DELAY) {
            let is_exit = msg == Msg::Exit;
            multiview.manage_msg(msg)?;
            if is_exit {
                break;
            }
        }

        let new_term_size = terminal_size()?;

        if term_size != new_term_size {
            term_size = new_term_size;

            for (i, row) in multiview.tiles.iter_mut().enumerate() {
                let row_len = row.len() as u16;

                let tile_size = if is_row_major {
                    (term_size.0 / row_len, term_size.1 / col_len)
                } else {
                    (term_size.0 / col_len, term_size.1 / row_len)
                };

                for (j, tile) in row.iter_mut().enumerate() {
                    let (p_i, p_j) = if is_row_major { (i, j) } else { (j, i) };
                    tile.reposition((p_j as u16 * tile_size.0 + 1, p_i as u16 * tile_size.1 + 1));
                    tile.resize(tile_size);
                }
            }

            multiview.refresh_tiles = true;
            multiview.refresh_ui = true;
        }

        multiview.render(false)?;
    }

    Ok(())
}
