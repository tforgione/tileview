fn main() {
    if let Err(e) = multiview::main() {
        eprintln!("An error occured: {}", e);
    }
}
