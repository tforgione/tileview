fn main() {
    if let Err(e) = tileview::main() {
        eprintln!("An error occured: {}", e);
    }
}
