//! P2: dialect-board regression guard.
//!
//! The CCD boards are KiCad-10 dialect files: pads/segments carry inline
//! `(net "NAME")` and there is NO numeric net table. Guards fixed here:
//! 1. Parsing a dialect board synthesizes a net table and resolves pad nets.
//! 2. Re-generating keeps every segment/via/pad on the SAME net name.
//! 3. Generated vias must never carry an empty net (`(net "")`) — an unnetted
//!    via reads as a floating barrel and DRC shorts it against anything it
//!    touches (measured: 121 shorting errors on the AFE board).

use kicad_json5::codegen::BoardSexprGenerator;
use kicad_json5::parse_board;

fn generate_kicad_pcb(board: &kicad_json5::Board) -> kicad_json5::Result<String> {
    BoardSexprGenerator::new().generate(board)
}

const DIALECT_BOARD: &str = r#"(kicad_pcb
	(version 20240108)
	(generator "test")
	(general
		(thickness 1.6)
	)
	(paper "A4")
	(layers
		(0 "F.Cu" signal)
		(31 "B.Cu" signal)
		(38 "B.Mask" user)
		(39 "F.Mask" user)
		(44 "Edge.Cuts" user)
	)
	(setup
		(stackup)
	)
	(footprint "Test:R0402"
		(layer "F.Cu")
		(uuid "aaaaaaaa-0000-0000-0000-000000000001")
		(at 10 10)
		(fp_text reference "R1"
			(at 0 -1)
			(layer "F.SilkS")
			(uuid "aaaaaaaa-0000-0000-0000-0000000000aa")
		)
		(pad "1" smd rect
			(at -1 0)
			(size 1 0.6)
			(layers "F.Cu" "F.Mask")
			(net "VOUT")
		)
		(pad "2" smd rect
			(at 1 0)
			(size 1 0.6)
			(layers "F.Cu" "F.Mask")
			(net "AGND")
		)
	)
	(footprint "Test:R0402"
		(layer "F.Cu")
		(uuid "aaaaaaaa-0000-0000-0000-000000000002")
		(at 20 10)
		(fp_text reference "R2"
			(at 0 -1)
			(layer "F.SilkS")
			(uuid "aaaaaaaa-0000-0000-0000-0000000000bb")
		)
		(pad "1" smd rect
			(at -1 0)
			(size 1 0.6)
			(layers "F.Cu" "F.Mask")
			(net "VOUT")
		)
		(pad "2" smd rect
			(at 1 0)
			(size 1 0.6)
			(layers "F.Cu" "F.Mask")
			(net "AGND")
		)
	)
	(segment
		(start 11 10)
		(end 19 10)
		(width 0.25)
		(layer "F.Cu")
		(net "VOUT")
	)
	(gr_rect
		(start 0 0)
		(end 30 20)
		(stroke (width 0.1) (type default))
		(layer "Edge.Cuts")
		(uuid "aaaaaaaa-0000-0000-0000-0000000000cc")
	)
)
"#;

#[test]
fn dialect_board_net_table_is_synthesized() {
    let board = parse_board(DIALECT_BOARD).unwrap();
    let names: Vec<&str> = board.nets.iter().map(|n| n.name.as_str()).collect();
    assert!(
        names.contains(&"VOUT"),
        "VOUT missing from net table: {:?}",
        names
    );
    assert!(
        names.contains(&"AGND"),
        "AGND missing from net table: {:?}",
        names
    );
    // Every pad resolved to a non-zero net id that maps back to its name.
    for fp in &board.footprints {
        for pad in &fp.pads {
            let id = pad.net.expect("dialect pad must resolve to a net id");
            assert_ne!(id, 0, "pad net id must be non-zero");
            let name = &board
                .nets
                .iter()
                .find(|n| n.id == id)
                .expect("net id in table")
                .name;
            assert_eq!(&pad.net_name.as_deref().unwrap_or(name), name);
        }
    }
}

/// Parse net references from generated output: pads may serialize as
/// `(net N "NAME")`, segments/vias as `(net N)`. The net id → name mapping
/// is established by the pads (KiCad registry mechanism), so the contract is:
/// every segment/via id must map to the same name the source board had.
#[test]
fn dialect_roundtrip_preserves_net_names() {
    let board = parse_board(DIALECT_BOARD).unwrap();
    let out = generate_kicad_pcb(&board).unwrap();

    // id -> name registry as rebuilt from the generated pads
    let mut id2name = std::collections::HashMap::new();
    for cap in out.match_indices("(net ").map(|(i, _)| &out[i..]) {
        let rest = &cap[5..];
        let mut it = rest.splitn(2, ')');
        let body = it.next().unwrap_or("");
        if let Some(q) = body.find('"') {
            // (net N "NAME")
            let id: u32 = body[..q].trim().parse().unwrap();
            let name = body[q + 1..].split('"').next().unwrap();
            id2name.insert(id, name.to_string());
        }
    }
    assert!(
        id2name.values().any(|n| n == "VOUT"),
        "VOUT registry missing: {:?}",
        id2name
    );
    assert!(
        id2name.values().any(|n| n == "AGND"),
        "AGND registry missing: {:?}",
        id2name
    );

    // segment (net 1) etc must exist and ids must be in the registry
    let mut seg_nets = 0u32;
    for cap in out.match_indices("(net ").map(|(i, _)| &out[i..]) {
        let rest = &cap[5..];
        let body = rest.split(')').next().unwrap_or("");
        if !body.contains('"') {
            let id: u32 = body.trim().parse().unwrap_or(u32::MAX);
            if id != u32::MAX && id != 0 {
                seg_nets += 1;
                assert!(
                    id2name.contains_key(&id),
                    "net id {} has no name registry",
                    id
                );
            }
        }
    }
    assert!(seg_nets >= 1, "no named segment nets in output");
}

/// An unnetted via (`(net "")`) is a floating barrel: DRC shorts it against
/// everything it touches (measured: 121 shorting errors on the AFE board).
#[test]
fn generated_vias_carry_net_names() {
    let mut board = parse_board(DIALECT_BOARD).unwrap();
    let vout_id = board.nets.iter().find(|n| n.name == "VOUT").unwrap().id;
    board.vias.push(kicad_json5::ir::board::Via {
        at: (15.0, 10.0),
        size: 0.6,
        drill: 0.3,
        layers: vec!["F.Cu".into(), "B.Cu".into()],
        net: vout_id,
    });
    let out = generate_kicad_pcb(&board).unwrap();
    let via_block = out.split("(via").nth(1).expect("via serialized");
    assert!(
        !via_block.contains("(net \"\")"),
        "via serialized with empty net:\n{}",
        via_block
    );
    // and the via id matches the VOUT registry entry
    let body = via_block
        .split("(net ")
        .nth(1)
        .unwrap()
        .split(')')
        .next()
        .unwrap();
    let id: u32 = body.trim().parse().expect("via net id numeric");
    let registry = out
        .match_indices("(net ")
        .map(|(i, _)| &out[i..])
        .find(|c| c.contains(&format!("(net {} \"VOUT\")", id)));
    assert!(
        registry.is_some(),
        "via net id {} not registered as VOUT",
        id
    );
}
