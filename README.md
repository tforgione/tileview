# tileview

*Run tiled commands in a single terminal*

![tileview preview](/screenshots/row-major.png)

## Installation

If rust is not already installed, [install rust](https://www.rust-lang.org/tools/install).

Then run:

```sh
cargo install tileview
```

## Usage

Split your terminal in two rows, the first containing three columns, and the second containing one column:
```sh
tileview cmd1 :: cmd2 :: cmd3 // cmd4 :: cmd5
```

![tileview row major preview](/screenshots/row-major.png)

Split your terminal in two columns, the first containing three rows, and the second containing one row:
```sh
tileview cmd1 // cmd2 // cmd3 :: cmd4 // cmd5
```

![tileview col major preview](/screenshots/col-major.png)

## Colors

Most well written programs will disable colors when running from tileview, in order to force them to use colors, you
can use the `unbuffer` command from the [`expect` package](https://packages.ubuntu.com/search?keywords=expect).

```sh
tileview unbuffer cmd1 :: unbuffer cmd2
```

## Shortcuts

  - `k`: kills the current tile
  - `K`: kills all tiles
  - `r`: restarts the current tile
  - `R`: restarts all tiles
  - `l`: draw a line on the current tile
  - `L`: draw a line on all tiles
  - `q`: quits

## History

This is my attempt to rewrite [arjunmehta's multiview](https://github.com/arjunmehta/multiview) in rust.

Their version has many features that I don't use, but is missing a few things that I need:
  - line wrapping: when a line is bigger than the terminal size, the end is just not displayed
  - scroll: if your output has more lines than your terminal height, there is no way (to my knowledge) to scroll up

