//! Some helper functions.

use termion::cursor;

/// Draws a box from (x1, y1) to (x2, y2).
pub fn rect((x1, y1): (u16, u16), (x2, y2): (u16, u16)) -> String {
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
