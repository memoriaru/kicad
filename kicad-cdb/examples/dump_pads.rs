fn main() {
    let path = std::env::args().nth(1).unwrap();
    let refdes = std::env::args().nth(2).unwrap();
    let board = kicad_json5::parse_board(&std::fs::read_to_string(&path).unwrap()).unwrap();
    for f in &board.footprints {
        if f.reference != refdes { continue; }
        println!("{} {} pads={}", f.reference, f.value, f.pads.len());
        for p in &f.pads {
            println!("  pin {:>3} -> {:?}", p.number, p.net_name.clone().unwrap_or_default());
        }
    }
}
