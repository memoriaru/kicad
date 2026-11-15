fn main() {
    let path = std::env::args().nth(1).unwrap();
    let want = std::env::args().nth(2).unwrap().to_uppercase();
    let board = kicad_json5::parse_board(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for f in &board.footprints {
        let pins: Vec<&String> = f.pads.iter().filter_map(|p| p.net_name.as_ref())
            .filter(|n| n.to_uppercase() == want).collect();
        if !pins.is_empty() {
            println!("{:<6} {:<24} {} pin(s) on {}", f.reference, f.value,
                f.lib_id.rsplit(':').next().unwrap_or(""), pins.len());
        }
    }
}
