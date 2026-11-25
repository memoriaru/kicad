fn main() {
    let path = std::env::args().nth(1).unwrap();
    let board = kicad_json5::parse_board(&std::fs::read_to_string(&path).unwrap()).unwrap();
    let mut names: Vec<String> = board.nets.iter().map(|n| n.name.clone()).collect();
    names.sort();
    println!("nets({}): {:?}", names.len(), names);
    let mut vrms: Vec<String> = board
        .footprints
        .iter()
        .map(|f| {
            format!(
                "{}={}:{}",
                f.reference,
                f.value,
                f.lib_id.split(':').next_back().unwrap_or("")
            )
        })
        .filter(|s| regex_lite(s))
        .collect();
    fn regex_lite(s: &str) -> bool {
        let l = s.to_lowercase();
        [
            "tps", "sy8", "ams", "rt9", "1117", "mp", "ldo", "buck", "boost",
        ]
        .iter()
        .any(|k| l.contains(k))
    }
    vrms.sort();
    println!("VRM candidates:\n{}", vrms.join("\n"));
}
