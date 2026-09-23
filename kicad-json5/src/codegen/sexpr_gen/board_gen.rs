//! S-expression code generator for KiCad PCB files (.kicad_pcb)

use super::KicadVersion;
use crate::error::Result;
use crate::ir::board::*;

/// PCB S-expression generator configuration
#[derive(Debug, Clone)]
pub struct BoardSexprConfig {
    /// Indentation string (default: "\t")
    pub indent: String,
    /// Whether to include UUIDs in output
    pub include_uuids: bool,
    /// Auto-generate UUIDs if missing
    pub generate_uuids: bool,
    /// Target KiCad version (None = auto-detect from input)
    pub kicad_version: Option<KicadVersion>,
    /// Net-reference emission dialect. `Official` (default) writes a top-level
    /// net table plus KiCad-canonical references — loadable by stock kicad-cli.
    /// `Huaqiu` inlines net names on elements and omits the table, which the
    /// HQ fork requires (its netcode allocator treats the two forms differently).
    pub dialect: crate::dialect::VendorDialect,
}

impl Default for BoardSexprConfig {
    fn default() -> Self {
        Self {
            indent: "\t".to_string(),
            include_uuids: true,
            generate_uuids: true,
            kicad_version: None,
            dialect: crate::dialect::VendorDialect::Official,
        }
    }
}

/// S-expression generator for `.kicad_pcb` files
pub struct BoardSexprGenerator {
    config: BoardSexprConfig,
    indent_level: usize,
    /// Effective version detected from input, used for version-specific output
    effective_version: KicadVersion,
    /// P0-10: deterministic uuid counter — same board content ⇒ byte-identical
    /// output (uuids only need uniqueness within a file, not global randomness)
    uuid_counter: u64,
}

impl BoardSexprGenerator {
    pub fn new() -> Self {
        Self {
            config: BoardSexprConfig::default(),
            indent_level: 0,
            effective_version: KicadVersion::V7,
            uuid_counter: 0,
        }
    }

    pub fn with_config(config: BoardSexprConfig) -> Self {
        Self {
            config,
            indent_level: 0,
            effective_version: KicadVersion::V7,
            uuid_counter: 0,
        }
    }

    pub fn generate(&mut self, board: &Board) -> Result<String> {
        // Auto-detect version from board IR
        self.effective_version = self.config.kicad_version.unwrap_or({
            match board.version.as_str() {
                "20231120" => KicadVersion::V8,
                "20250114" => KicadVersion::V9,
                "20260306" => KicadVersion::V10,
                _ => KicadVersion::V7,
            }
        });

        let mut out = String::new();

        out.push_str("(kicad_pcb\n");
        self.indent_level = 1;

        // Version & generator
        self.line(&mut out, &format!("(version {})", board.version));
        self.line(
            &mut out,
            &format!("(generator \"{}\")", Self::esc(&board.generator)),
        );

        // General
        self.write_general(&mut out, board);

        // Paper: standard sizes are quoted ("A4"); KiCad requires
        // (paper "User" W H) with "User" quoted — unquoted breaks kicad-cli.
        // P0-4: content overflowing the declared page gets clipped in every
        // render/export — enlarge the page to fit the actual board extent.
        let (mut pw, mut ph) = if board.paper.starts_with("User ") {
            let mut it = board.paper.trim_start_matches("User ").split_whitespace();
            (
                it.next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(297.0),
                it.next()
                    .and_then(|v| v.parse::<f64>().ok())
                    .unwrap_or(210.0),
            )
        } else if board.paper == "A3" {
            (420.0, 297.0)
        } else if board.paper == "A5" {
            (210.0, 148.5)
        } else {
            (297.0, 210.0)
        };
        let mut max_x = 0.0f64;
        let mut max_y = 0.0f64;
        for fp in &board.footprints {
            max_x = max_x.max(fp.position.0 + 4.0);
            max_y = max_y.max(fp.position.1 + 4.0);
        }
        for s in &board.segments {
            max_x = max_x.max(s.start.0.max(s.end.0) + 2.0);
            max_y = max_y.max(s.start.1.max(s.end.1) + 2.0);
        }
        for v in &board.vias {
            max_x = max_x.max(v.at.0 + 2.0);
            max_y = max_y.max(v.at.1 + 2.0);
        }
        for g in &board.graphics {
            use BoardGraphicKind::*;
            let pts: Vec<(f64, f64)> = match &g.kind {
                Line { start, end } => vec![*start, *end],
                Arc { start, mid, end } => vec![*start, *mid, *end],
                Circle { center, end } => vec![*center, *end],
                Rect { start, end } => vec![*start, *end],
                Poly { points } => points.clone(),
                Text { position, .. } => vec![(position.0, position.1)],
            };
            for p in pts {
                max_x = max_x.max(p.0 + 2.0);
                max_y = max_y.max(p.1 + 2.0);
            }
        }
        if max_x > pw || max_y > ph {
            pw = (max_x + 2.0).ceil();
            ph = (max_y + 2.0).ceil();
            self.line(&mut out, &format!("(paper \"User\" {} {})", pw, ph));
        } else if board.paper.starts_with("User ") {
            let dims = board.paper.trim_start_matches("User ");
            self.line(&mut out, &format!("(paper \"User\" {})", dims));
        } else {
            self.line(&mut out, &format!("(paper \"{}\")", board.paper));
        }

        // Title block
        if let Some(ref tb) = board.title_block {
            self.write_title_block(&mut out, tb);
        }

        // Layers
        self.write_layers(&mut out, &board.layers);

        // Setup
        self.write_setup(&mut out, &board.setup);

        // Nets: 华秋 fork 下不写声明块——声明块的数字 id 与元素字符串名
        // 各走一条 netcode 分配路径，会让 zone 把同网 via/pad 判异网挖空。
        // net 名内联在各元素 (net "NAME") 上，同名即同网。

        // Official 方言必须写声明表, 元素按官方规范引用 (pad 双元带名,
        // segment/via 纯数字)——stock kicad-cli 拒载缺表/内联名的文件。
        if self.config.dialect == crate::dialect::VendorDialect::Official {
            for net in &board.nets {
                self.line(
                    &mut out,
                    &format!("(net {} \"{}\")", net.id, Self::esc(&net.name)),
                );
            }
        }

        // Build net id→name lookup for pad/segment/via references
        let net_names: std::collections::HashMap<u32, String> =
            board.nets.iter().map(|n| (n.id, n.name.clone())).collect();

        // Footprints
        for fp in &board.footprints {
            self.write_footprint(&mut out, fp, &net_names);
        }

        // Graphics (board-level)
        for gr in &board.graphics {
            self.write_board_graphic(&mut out, gr);
        }

        // Segments
        for seg in &board.segments {
            self.write_segment(&mut out, seg, &net_names);
        }

        // Vias
        for via in &board.vias {
            self.write_via(&mut out, via, &net_names);
        }

        // Zones
        for zone in &board.zones {
            self.write_zone(&mut out, zone);
        }

        self.indent_level = 0;
        out.push_str(")\n");

        Ok(out)
    }

    // ── Helpers ──────────────────────────────────────────────────────────

    fn indent(&self) -> String {
        self.config.indent.repeat(self.indent_level)
    }

    fn line(&self, out: &mut String, content: &str) {
        out.push_str(&self.indent());
        out.push_str(content);
        out.push('\n');
    }

    fn fmt_num(n: f64) -> String {
        let n = (n * 1e6).round() / 1e6;
        if (n - n.round()).abs() < 1e-9 {
            format!("{}", n.round() as i64)
        } else {
            format!("{}", n)
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_string()
        }
    }

    fn fmt_xy(x: f64, y: f64) -> String {
        format!("(xy {} {})", Self::fmt_num(x), Self::fmt_num(y))
    }

    fn fmt_at(x: f64, y: f64, r: f64) -> String {
        if r == 0.0 {
            format!("(at {} {})", Self::fmt_num(x), Self::fmt_num(y))
        } else {
            format!(
                "(at {} {} {})",
                Self::fmt_num(x),
                Self::fmt_num(y),
                Self::fmt_num(r)
            )
        }
    }

    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }

    fn uuid(&mut self) -> String {
        // P0-10: splitmix64 from a fixed seed + counter, formatted as v4
        // (version/variant bits set) — deterministic across runs.
        self.uuid_counter += 1;
        let mut z = 0x9E3779B97F4A7C15u64 ^ self.uuid_counter.wrapping_mul(0xBF58476D1CE4E5B9);
        z = z.wrapping_add(0x9E3779B97F4A7C15);
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
        z ^= z >> 31;
        let h2 = z.wrapping_mul(0x2545F4914245C3C9) ^ 0xDEADBEEFCAFEBABE;
        let mut bytes = [0u8; 16];
        #[allow(clippy::needless_range_loop)] // bit-stuffing, clearest indexed
        for i in 0..8 {
            bytes[i] = ((z >> (i * 8)) & 0xFF) as u8;
        }
        for i in 0..8 {
            bytes[8 + i] = ((h2 >> (i * 8)) & 0xFF) as u8;
        }
        bytes[6] = (bytes[6] & 0x0F) | 0x40;
        bytes[8] = (bytes[8] & 0x3F) | 0x80;
        let hex: String = bytes.iter().map(|x| format!("{:02x}", x)).collect();
        format!(
            "{}-{}-{}-{}-{}",
            &hex[0..8],
            &hex[8..12],
            &hex[12..16],
            &hex[16..20],
            &hex[20..32]
        )
    }

    fn maybe_uuid(&mut self, out: &mut String) {
        if self.config.include_uuids && self.config.generate_uuids {
            let u = self.uuid();
            self.line(out, &format!("(uuid \"{}\")", u));
        }
    }

    // ── Sections ─────────────────────────────────────────────────────────

    fn write_general(&mut self, out: &mut String, board: &Board) {
        self.line(out, "(general");
        self.indent_level += 1;
        self.line(
            out,
            &format!("(thickness {})", Self::fmt_num(board.general.thickness)),
        );
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_title_block(&mut self, out: &mut String, tb: &BoardTitleBlock) {
        self.line(out, "(title_block");
        self.indent_level += 1;
        if let Some(ref t) = tb.title {
            self.line(out, &format!("(title \"{}\")", Self::esc(t)));
        }
        if let Some(ref d) = tb.date {
            self.line(out, &format!("(date \"{}\")", d));
        }
        if let Some(ref r) = tb.rev {
            self.line(out, &format!("(rev \"{}\")", r));
        }
        if let Some(ref c) = tb.company {
            self.line(out, &format!("(company \"{}\")", Self::esc(c)));
        }
        for (i, txt) in &tb.comment {
            self.line(out, &format!("(comment {} \"{}\")", i + 1, Self::esc(txt)));
        }
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_layers(&mut self, out: &mut String, layers: &[LayerDef]) {
        self.line(out, "(layers");
        self.indent_level += 1;
        for l in layers {
            self.line(
                out,
                &format!("({} \"{}\" {})", l.ordinal, l.name, l.layer_type),
            );
        }
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_setup(&mut self, out: &mut String, setup: &BoardSetup) {
        self.line(out, "(setup");
        self.indent_level += 1;

        // Stackup
        if !setup.stackup.is_empty() {
            self.line(out, "(stackup");
            self.indent_level += 1;
            for sl in &setup.stackup {
                self.line(
                    out,
                    &format!("(layer \"{}\" (type {}))", sl.name, sl.layer_type),
                );
            }
            self.indent_level -= 1;
            self.line(out, ")");
        }

        self.line(
            out,
            &format!(
                "(pad_to_mask_clearance {})",
                Self::fmt_num(setup.pad_to_mask_clearance)
            ),
        );
        self.line(
            out,
            &format!(
                "(pad_to_paste_clearance {})",
                Self::fmt_num(setup.pad_to_paste_clearance)
            ),
        );
        self.line(
            out,
            &format!(
                "(pad_to_paste_clearance_ratio {})",
                Self::fmt_num(setup.pad_to_paste_clearance_ratio)
            ),
        );

        if let Some((x, y)) = setup.aux_axis_origin {
            self.line(
                out,
                &format!(
                    "(aux_axis_origin {} {})",
                    Self::fmt_num(x),
                    Self::fmt_num(y)
                ),
            );
        }
        if let Some((x, y)) = setup.grid_origin {
            self.line(
                out,
                &format!("(grid_origin {} {})", Self::fmt_num(x), Self::fmt_num(y)),
            );
        }

        self.indent_level -= 1;
        self.line(out, ")");
    }

    // ── Footprint ────────────────────────────────────────────────────────

    fn write_footprint(
        &mut self,
        out: &mut String,
        fp: &Footprint,
        net_names: &std::collections::HashMap<u32, String>,
    ) {
        self.line(out, &format!("(footprint \"{}\"", Self::esc(&fp.lib_id)));
        self.indent_level += 1;

        if fp.locked {
            self.line(out, "(locked yes)");
        }
        self.line(out, &format!("(layer \"{}\")", fp.layer));
        if let Some(ref uuid) = fp.uuid {
            self.line(out, &format!("(uuid \"{}\")", uuid));
        } else {
            self.maybe_uuid(out);
        }
        let (x, y, r) = fp.position;
        self.line(out, &Self::fmt_at(x, y, r));

        if let Some(ref descr) = fp.descr {
            self.line(out, &format!("(descr \"{}\")", Self::esc(descr)));
        }
        if let Some(ref tags) = fp.tags {
            self.line(out, &format!("(tags \"{}\")", Self::esc(tags)));
        }

        // Properties (full format with effects)
        if !fp.properties_ext.is_empty() {
            for prop in &fp.properties_ext {
                self.write_fp_property(out, prop);
            }
        } else {
            // Legacy: simple key-value as fp_text — skip reference/value when
            // fp_texts already carries them (P2-4 dedup)
            let has_ref_t = fp
                .fp_texts
                .iter()
                .any(|t| t.text_type == FpTextType::Reference);
            let has_val_t = fp.fp_texts.iter().any(|t| t.text_type == FpTextType::Value);
            if !has_ref_t {
                self.write_fp_text(out, "reference", &fp.reference, (0.0, -1.0, 0.0), "F.SilkS");
            }
            if !has_val_t {
                self.write_fp_text(out, "value", &fp.value, (0.0, 1.0, 0.0), "F.SilkS");
            }
            for (k, v) in &fp.properties {
                self.write_fp_text(out, k, v, (0.0, 0.0, 0.0), "F.Fab");
            }
        }

        if let Some(ref path) = fp.path {
            self.line(out, &format!("(path \"{}\")", path));
        }
        if let Some(ref sn) = fp.sheetname {
            self.line(out, &format!("(sheetname \"{}\")", Self::esc(sn)));
        }
        if let Some(ref sf) = fp.sheetfile {
            self.line(out, &format!("(sheetfile \"{}\")", Self::esc(sf)));
        }
        if let Some(ref attr) = fp.attr {
            match attr {
                FootprintAttr::Smd => self.line(out, "(attr smd)"),
                FootprintAttr::ThruHole => self.line(out, "(attr thru_hole)"),
                FootprintAttr::Virtual => self.line(out, "(attr virtual)"),
                FootprintAttr::BoardOnly => self.line(out, "(attr board_only)"),
                FootprintAttr::Other(vals) => {
                    self.line(out, &format!("(attr {})", vals.join(" ")));
                }
            }
        }
        if let Some(margin) = fp.solder_mask_margin {
            self.line(
                out,
                &format!("(solder_mask_margin {})", Self::fmt_num(margin)),
            );
        }
        if let Some(margin) = fp.solder_paste_margin {
            self.line(
                out,
                &format!("(solder_paste_margin {})", Self::fmt_num(margin)),
            );
        }
        if let Some(ratio) = fp.solder_paste_ratio {
            self.line(
                out,
                &format!("(solder_paste_ratio {})", Self::fmt_num(ratio)),
            );
        }
        if let Some(cl) = fp.clearance {
            self.line(out, &format!("(clearance {})", Self::fmt_num(cl)));
        }
        if let Some(zc) = fp.zone_connect {
            self.line(out, &format!("(zone_connect {})", zc));
        }
        if let Some(tw) = fp.thermal_bridge_width {
            self.line(
                out,
                &format!("(thermal_bridge_width {})", Self::fmt_num(tw)),
            );
        }
        if let Some(ta) = fp.thermal_bridge_angle {
            self.line(
                out,
                &format!("(thermal_bridge_angle {})", Self::fmt_num(ta)),
            );
        }
        if let Some(tg) = fp.thermal_gap {
            self.line(out, &format!("(thermal_gap {})", Self::fmt_num(tg)));
        }

        // fp_line graphics
        for fl in &fp.fp_lines {
            self.write_fp_line(out, fl);
        }
        for fc in &fp.fp_circles {
            self.write_fp_circle(out, fc);
        }
        for fa in &fp.fp_arcs {
            self.write_fp_arc(out, fa);
        }
        for fr in &fp.fp_rects {
            self.write_fp_rect(out, fr);
        }
        for fp in &fp.fp_polys {
            self.write_fp_poly(out, fp);
        }
        for ft in &fp.fp_texts {
            self.write_fp_text_sized(
                out,
                Self::fp_text_type_str(ft.text_type),
                &ft.text,
                ft.position,
                &ft.layer,
                ft.font_size,
                0.15,
            );
        }

        // Pads
        for pad in &fp.pads {
            self.write_pad(out, pad, net_names);
        }

        // Models（多个全量回写; 子字段显式带出 → 二次 convert 位级幂等）
        for model in &fp.models {
            self.write_model(out, model);
        }

        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_fp_property(&mut self, out: &mut String, prop: &FpProperty) {
        self.line(out, &format!("(property \"{}\"", Self::esc(&prop.name)));
        self.indent_level += 1;

        self.line(out, &format!("\"{}\"", Self::esc(&prop.value)));
        let (x, y, r) = prop.position;
        self.line(out, &Self::fmt_at(x, y, r));

        if prop.unlocked {
            self.line(out, "(unlocked yes)");
        }
        self.line(out, &format!("(layer \"{}\")", prop.layer));
        if prop.hide {
            self.line(out, "(hide yes)");
        }
        if let Some(ref uuid) = prop.uuid {
            self.line(out, &format!("(uuid \"{}\")", uuid));
        } else {
            self.maybe_uuid(out);
        }

        // effects
        let (fw, fh) = prop.effects.font_size;
        self.line(
            out,
            &format!(
                "(effects (font (size {} {}) (thickness {}){}{}))",
                Self::fmt_num(fw),
                Self::fmt_num(fh),
                Self::fmt_num(prop.effects.font_thickness),
                if prop.effects.bold { " (bold yes)" } else { "" },
                if prop.effects.italic {
                    " (italic yes)"
                } else {
                    ""
                },
            ),
        );

        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_model(&mut self, out: &mut String, model: &Model3D) {
        self.line(out, &format!("(model \"{}\"", Self::esc(&model.path)));
        self.indent_level += 1;
        let (ox, oy, oz) = model.offset;
        self.line(
            out,
            &format!(
                "(offset (xyz {} {} {}))",
                Self::fmt_num(ox),
                Self::fmt_num(oy),
                Self::fmt_num(oz)
            ),
        );
        let (sx, sy, sz) = model.scale;
        self.line(
            out,
            &format!(
                "(scale (xyz {} {} {}))",
                Self::fmt_num(sx),
                Self::fmt_num(sy),
                Self::fmt_num(sz)
            ),
        );
        let (rx, ry, rz) = model.rotate;
        self.line(
            out,
            &format!(
                "(rotate (xyz {} {} {}))",
                Self::fmt_num(rx),
                Self::fmt_num(ry),
                Self::fmt_num(rz)
            ),
        );
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn fp_text_type_str(t: FpTextType) -> &'static str {
        match t {
            FpTextType::Reference => "reference",
            FpTextType::Value => "value",
            FpTextType::User => "user",
        }
    }

    fn write_fp_text(
        &mut self,
        out: &mut String,
        text_type: &str,
        text: &str,
        pos: (f64, f64, f64),
        layer: &str,
    ) {
        self.write_fp_text_sized(out, text_type, text, pos, layer, (1.0, 1.0), 0.15);
    }

    #[allow(clippy::too_many_arguments)]
    fn write_fp_text_sized(
        &mut self,
        out: &mut String,
        text_type: &str,
        text: &str,
        pos: (f64, f64, f64),
        layer: &str,
        font_size: (f64, f64),
        thickness: f64,
    ) {
        self.line(
            out,
            &format!("(fp_text {} \"{}\"", text_type, Self::esc(text)),
        );
        self.indent_level += 1;
        let (x, y, r) = pos;
        self.line(out, &Self::fmt_at(x, y, r));
        self.line(out, &format!("(layer \"{}\")", layer));
        self.maybe_uuid(out);
        self.line(
            out,
            &format!(
                "(effects (font (size {} {}) (thickness {})))",
                Self::fmt_num(font_size.0),
                Self::fmt_num(font_size.1),
                Self::fmt_num(thickness)
            ),
        );
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_fp_line(&mut self, out: &mut String, fl: &FpLine) {
        self.line(out, "(fp_line");
        self.indent_level += 1;
        let (sx, sy) = fl.start;
        let (ex, ey) = fl.end;
        self.line(
            out,
            &format!("(start {} {})", Self::fmt_num(sx), Self::fmt_num(sy)),
        );
        self.line(
            out,
            &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
        );
        self.line(
            out,
            &format!(
                "(stroke (width {}) (type default))",
                Self::fmt_num(fl.stroke_width)
            ),
        );
        self.line(out, &format!("(layer \"{}\")", fl.layer));
        self.maybe_uuid(out);
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_fp_circle(&mut self, out: &mut String, fc: &FpCircle) {
        self.line(out, "(fp_circle");
        self.indent_level += 1;
        let (cx, cy) = fc.center;
        let (ex, ey) = fc.end;
        self.line(
            out,
            &format!("(center {} {})", Self::fmt_num(cx), Self::fmt_num(cy)),
        );
        self.line(
            out,
            &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
        );
        self.line(
            out,
            &format!(
                "(stroke (width {}) (type default))",
                Self::fmt_num(fc.stroke_width)
            ),
        );
        self.line(out, &format!("(layer \"{}\")", fc.layer));
        if fc.fill {
            self.line(out, "(fill solid)");
        }
        self.maybe_uuid(out);
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_fp_arc(&mut self, out: &mut String, fa: &FpArc) {
        self.line(out, "(fp_arc");
        self.indent_level += 1;
        let (sx, sy) = fa.start;
        let (mx, my) = fa.mid;
        let (ex, ey) = fa.end;
        self.line(
            out,
            &format!("(start {} {})", Self::fmt_num(sx), Self::fmt_num(sy)),
        );
        self.line(
            out,
            &format!("(mid {} {})", Self::fmt_num(mx), Self::fmt_num(my)),
        );
        self.line(
            out,
            &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
        );
        self.line(
            out,
            &format!(
                "(stroke (width {}) (type default))",
                Self::fmt_num(fa.stroke_width)
            ),
        );
        self.line(out, &format!("(layer \"{}\")", fa.layer));
        self.maybe_uuid(out);
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_fp_rect(&mut self, out: &mut String, fr: &FpRect) {
        self.line(out, "(fp_rect");
        self.indent_level += 1;
        let (sx, sy) = fr.start;
        let (ex, ey) = fr.end;
        self.line(
            out,
            &format!("(start {} {})", Self::fmt_num(sx), Self::fmt_num(sy)),
        );
        self.line(
            out,
            &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
        );
        self.line(
            out,
            &format!(
                "(stroke (width {}) (type default))",
                Self::fmt_num(fr.stroke_width)
            ),
        );
        self.line(out, &format!("(layer \"{}\")", fr.layer));
        if fr.fill {
            self.line(out, "(fill solid)");
        }
        self.maybe_uuid(out);
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_fp_poly(&mut self, out: &mut String, fp: &FpPoly) {
        self.line(out, "(fp_poly");
        self.indent_level += 1;
        self.line(out, "(pts");
        self.indent_level += 1;
        for &(px, py) in &fp.points {
            self.line(out, &Self::fmt_xy(px, py));
        }
        self.indent_level -= 1;
        self.line(out, ")");
        self.line(
            out,
            &format!(
                "(stroke (width {}) (type default))",
                Self::fmt_num(fp.stroke_width)
            ),
        );
        self.line(out, &format!("(layer \"{}\")", fp.layer));
        if fp.fill {
            self.line(out, "(fill solid)");
        }
        self.maybe_uuid(out);
        self.indent_level -= 1;
        self.line(out, ")");
    }

    // ── Pad ──────────────────────────────────────────────────────────────

    fn write_pad(
        &mut self,
        out: &mut String,
        pad: &Pad,
        net_names: &std::collections::HashMap<u32, String>,
    ) {
        self.line(
            out,
            &format!(
                "(pad \"{}\" {} {}",
                pad.number,
                pad.pad_type.as_str(),
                pad.shape.as_str()
            ),
        );
        self.indent_level += 1;
        let (x, y, r) = pad.position;
        self.line(out, &Self::fmt_at(x, y, r));
        let (w, h) = pad.size;
        self.line(
            out,
            &format!("(size {} {})", Self::fmt_num(w), Self::fmt_num(h)),
        );

        if let Some(ref drill) = pad.drill {
            match drill.width {
                Some(w) => self.line(
                    out,
                    &format!(
                        "(drill oval {} {})",
                        Self::fmt_num(w),
                        Self::fmt_num(drill.diameter)
                    ),
                ),
                None => self.line(out, &format!("(drill {})", Self::fmt_num(drill.diameter))),
            }
            if let Some((ox, oy)) = drill.offset {
                self.line(
                    out,
                    &format!("(offset {} {})", Self::fmt_num(ox), Self::fmt_num(oy)),
                );
            }
        }

        let layers_str = pad
            .layers
            .iter()
            .map(|l| format!("\"{}\"", l))
            .collect::<Vec<_>>()
            .join(" ");
        self.line(out, &format!("(layers {})", layers_str));

        if let Some(remove_unused_layers) = pad.remove_unused_layers {
            self.line(
                out,
                &format!(
                    "(remove_unused_layers {})",
                    if remove_unused_layers { "yes" } else { "no" }
                ),
            );
        }
        if let Some(roundrect_rratio) = pad.roundrect_rratio {
            self.line(
                out,
                &format!("(roundrect_rratio {})", Self::fmt_num(roundrect_rratio)),
            );
        }
        if let Some(net_id) = pad.net {
            if net_id > 0 {
                match self.config.dialect {
                    crate::dialect::VendorDialect::Official => {
                        // 官方: 双元 (net id "name")——名字内联, 无名回退空串
                        let name = net_names.get(&net_id).map(String::as_str).unwrap_or("");
                        self.line(out, &format!("(net {} \"{}\")", net_id, Self::esc(name)));
                    }
                    crate::dialect::VendorDialect::Huaqiu => {
                        // 华秋 fork 方言：pad 的 net 也是纯字符串——双元格式与
                        // segment/via/zone 的字符串 netcode 走不同分配路径，会把
                        // 同网 pad/zone 判成异网（refill 时 pad 周围填充被挖空）。
                        if let Some(name) = net_names.get(&net_id) {
                            if !name.is_empty() {
                                self.line(out, &format!("(net \"{}\")", Self::esc(name)));
                            } else {
                                self.line(out, &format!("(net {} \"\")", net_id));
                            }
                        } else {
                            self.line(out, &format!("(net {})", net_id));
                        }
                    }
                }
            } else {
                self.line(out, &format!("(net {} \"\")", net_id));
            }
        }
        if let Some(ref pf) = pad.pin_function {
            self.line(out, &format!("(pinfunction \"{}\")", Self::esc(pf)));
        }
        if let Some(ref pt) = pad.pin_type {
            self.line(out, &format!("(pintype \"{}\")", pt));
        }
        if let Some(margin) = pad.solder_mask_margin {
            self.line(
                out,
                &format!("(solder_mask_margin {})", Self::fmt_num(margin)),
            );
        }
        if let Some(tw) = pad.thermal_bridge_width {
            self.line(
                out,
                &format!("(thermal_bridge_width {})", Self::fmt_num(tw)),
            );
        }
        if let Some(ta) = pad.thermal_bridge_angle {
            self.line(
                out,
                &format!("(thermal_bridge_angle {})", Self::fmt_num(ta)),
            );
        }
        if let Some(tg) = pad.thermal_gap {
            self.line(out, &format!("(thermal_gap {})", Self::fmt_num(tg)));
        }
        if let Some(cl) = pad.clearance {
            self.line(out, &format!("(clearance {})", Self::fmt_num(cl)));
        }
        if let Some(zc) = pad.zone_connect {
            self.line(out, &format!("(zone_connect {})", zc));
        }
        if let Some(ref opts) = pad.options {
            self.line(out, "(options");
            self.indent_level += 1;
            if let Some(ref c) = opts.clearance {
                self.line(out, &format!("(clearance {})", c));
            }
            if let Some(ref a) = opts.anchor {
                self.line(out, &format!("(anchor {})", a));
            }
            self.indent_level -= 1;
            self.line(out, ")");
        }
        if !pad.primitives.is_empty() {
            self.line(out, "(primitives");
            self.indent_level += 1;
            for prim in &pad.primitives {
                self.write_pad_primitive(out, prim);
            }
            self.indent_level -= 1;
            self.line(out, ")");
        }

        self.maybe_uuid(out);
        self.indent_level -= 1;
        self.line(out, ")");
    }

    fn write_pad_primitive(&mut self, out: &mut String, prim: &crate::ir::board::PadPrimitive) {
        use crate::ir::board::PadPrimitive as P;
        let fmt_pts = |pts: &[(f64, f64)]| {
            pts.iter()
                .map(|(x, y)| format!("(xy {} {})", Self::fmt_num(*x), Self::fmt_num(*y)))
                .collect::<Vec<_>>()
                .join(" ")
        };
        match prim {
            P::GrPoly { pts, width, fill } => {
                self.line(out, "(gr_poly");
                self.indent_level += 1;
                self.line(out, &format!("(pts {})", fmt_pts(pts)));
                self.line(out, &format!("(width {})", Self::fmt_num(*width)));
                self.line(out, &format!("(fill {})", if *fill { "yes" } else { "no" }));
                self.indent_level -= 1;
                self.line(out, ")");
            }
            P::GrRect {
                start,
                end,
                width,
                fill,
            } => {
                self.line(out, "(gr_rect");
                self.indent_level += 1;
                self.line(
                    out,
                    &format!(
                        "(start {} {})",
                        Self::fmt_num(start.0),
                        Self::fmt_num(start.1)
                    ),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(end.0), Self::fmt_num(end.1)),
                );
                self.line(out, &format!("(width {})", Self::fmt_num(*width)));
                self.line(out, &format!("(fill {})", if *fill { "yes" } else { "no" }));
                self.indent_level -= 1;
                self.line(out, ")");
            }
            P::GrLine { start, end, width } => {
                self.line(out, "(gr_line");
                self.indent_level += 1;
                self.line(
                    out,
                    &format!(
                        "(start {} {})",
                        Self::fmt_num(start.0),
                        Self::fmt_num(start.1)
                    ),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(end.0), Self::fmt_num(end.1)),
                );
                self.line(out, &format!("(width {})", Self::fmt_num(*width)));
                self.indent_level -= 1;
                self.line(out, ")");
            }
            P::GrCircle {
                center,
                end,
                width,
                fill,
            } => {
                self.line(out, "(gr_circle");
                self.indent_level += 1;
                self.line(
                    out,
                    &format!(
                        "(center {} {})",
                        Self::fmt_num(center.0),
                        Self::fmt_num(center.1)
                    ),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(end.0), Self::fmt_num(end.1)),
                );
                self.line(out, &format!("(width {})", Self::fmt_num(*width)));
                self.line(out, &format!("(fill {})", if *fill { "yes" } else { "no" }));
                self.indent_level -= 1;
                self.line(out, ")");
            }
            P::GrArc {
                start,
                mid,
                end,
                width,
            } => {
                self.line(out, "(gr_arc");
                self.indent_level += 1;
                self.line(
                    out,
                    &format!(
                        "(start {} {})",
                        Self::fmt_num(start.0),
                        Self::fmt_num(start.1)
                    ),
                );
                self.line(
                    out,
                    &format!("(mid {} {})", Self::fmt_num(mid.0), Self::fmt_num(mid.1)),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(end.0), Self::fmt_num(end.1)),
                );
                self.line(out, &format!("(width {})", Self::fmt_num(*width)));
                self.indent_level -= 1;
                self.line(out, ")");
            }
            P::Segment { start, end, width } => {
                self.line(out, "(segment");
                self.indent_level += 1;
                self.line(
                    out,
                    &format!(
                        "(start {} {})",
                        Self::fmt_num(start.0),
                        Self::fmt_num(start.1)
                    ),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(end.0), Self::fmt_num(end.1)),
                );
                self.line(out, &format!("(width {})", Self::fmt_num(*width)));
                self.indent_level -= 1;
                self.line(out, ")");
            }
        }
    }

    // ── Segment ──────────────────────────────────────────────────────────

    fn write_segment(
        &mut self,
        out: &mut String,
        seg: &Segment,
        net_names: &std::collections::HashMap<u32, String>,
    ) {
        self.line(out, "(segment");
        self.indent_level += 1;
        let (sx, sy) = seg.start;
        let (ex, ey) = seg.end;
        self.line(
            out,
            &format!("(start {} {})", Self::fmt_num(sx), Self::fmt_num(sy)),
        );
        self.line(
            out,
            &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
        );
        self.line(out, &format!("(width {})", Self::fmt_num(seg.width)));
        self.line(out, &format!("(layer \"{}\")", seg.layer));
        // 华秋 fork 方言：segment/via 的 net 是纯字符串 (net "name")——
        // 双元 (net id "name") 会被该 fork 拒载（"应为 ')'"）。无名字时退回数字。
        // Official: 纯数字引用顶层 net 表。
        match self.config.dialect {
            crate::dialect::VendorDialect::Official => {
                self.line(out, &format!("(net {})", seg.net));
            }
            crate::dialect::VendorDialect::Huaqiu => match net_names.get(&seg.net) {
                Some(name) if !name.is_empty() => {
                    self.line(out, &format!("(net \"{}\")", Self::esc(name)))
                }
                _ => self.line(out, &format!("(net {})", seg.net)),
            },
        }
        self.maybe_uuid(out);
        self.indent_level -= 1;
        self.line(out, ")");
    }

    // ── Via ──────────────────────────────────────────────────────────────

    fn write_via(
        &mut self,
        out: &mut String,
        via: &Via,
        net_names: &std::collections::HashMap<u32, String>,
    ) {
        self.line(out, "(via");
        self.indent_level += 1;
        let (x, y) = via.at;
        self.line(out, &Self::fmt_at(x, y, 0.0));
        self.line(out, &format!("(size {})", Self::fmt_num(via.size)));
        self.line(out, &format!("(drill {})", Self::fmt_num(via.drill)));
        let layers_str = via
            .layers
            .iter()
            .map(|l| format!("\"{}\"", l))
            .collect::<Vec<_>>()
            .join(" ");
        self.line(out, &format!("(layers {})", layers_str));
        // Official: 纯数字; 华秋 fork: 内联字符串名 (同 segment)。
        match self.config.dialect {
            crate::dialect::VendorDialect::Official => {
                self.line(out, &format!("(net {})", via.net));
            }
            crate::dialect::VendorDialect::Huaqiu => match net_names.get(&via.net) {
                Some(name) if !name.is_empty() => {
                    self.line(out, &format!("(net \"{}\")", Self::esc(name)))
                }
                _ => self.line(out, &format!("(net {})", via.net)),
            },
        }
        self.maybe_uuid(out);
        self.indent_level -= 1;
        self.line(out, ")");
    }

    // ── Zone ─────────────────────────────────────────────────────────────

    fn write_zone(&mut self, out: &mut String, zone: &Zone) {
        self.line(out, "(zone");
        self.indent_level += 1;
        // 华秋 fork 方言：zone 头用纯字符串 (net "name")——双元 (net id)+(net_name)
        // 会被该 fork 当作无网 zone，refill 只出碎片填充。
        // Official: (net id) + (net_name "...") 分立两字段 (KiCad 规范)。
        match self.config.dialect {
            crate::dialect::VendorDialect::Official => {
                self.line(out, &format!("(net {})", zone.net));
                if !zone.net_name.is_empty() {
                    self.line(
                        out,
                        &format!("(net_name \"{}\")", Self::esc(&zone.net_name)),
                    );
                }
            }
            crate::dialect::VendorDialect::Huaqiu => {
                if !zone.net_name.is_empty() {
                    self.line(out, &format!("(net \"{}\")", Self::esc(&zone.net_name)));
                } else {
                    self.line(out, &format!("(net {})", zone.net));
                }
            }
        }
        self.line(out, &format!("(layer \"{}\")", zone.layer));
        self.maybe_uuid(out);
        self.line(
            out,
            &format!(
                "(hatch {} {})",
                zone.hatch_style,
                Self::fmt_num(zone.hatch_pitch)
            ),
        );

        // connect_pads — 空串=省略 token (KiCad 默认 thermal, 华秋 fork 拒收裸 thermal);
        // 显式词 (yes/none/thru_hole_only/full) 原样带出防 solid 退化
        if zone.pad_connect.is_empty() {
            self.line(
                out,
                &format!(
                    "(connect_pads (clearance {}))",
                    Self::fmt_num(zone.connect_pads_clearance)
                ),
            );
        } else {
            self.line(
                out,
                &format!(
                    "(connect_pads {} (clearance {}))",
                    zone.pad_connect,
                    Self::fmt_num(zone.connect_pads_clearance)
                ),
            );
        }

        self.line(
            out,
            &format!("(min_thickness {})", Self::fmt_num(zone.min_thickness)),
        );

        // keepout
        if let Some(ref ko) = zone.keepout {
            self.line(out, "(keepout");
            self.indent_level += 1;
            self.line(
                out,
                &format!(
                    "(tracks {})",
                    if ko.tracks { "allowed" } else { "not_allowed" }
                ),
            );
            self.line(
                out,
                &format!("(vias {})", if ko.vias { "allowed" } else { "not_allowed" }),
            );
            self.line(
                out,
                &format!("(pads {})", if ko.pads { "allowed" } else { "not_allowed" }),
            );
            self.line(
                out,
                &format!(
                    "(copperpour {})",
                    if ko.copperpour {
                        "allowed"
                    } else {
                        "not_allowed"
                    }
                ),
            );
            self.line(
                out,
                &format!(
                    "(footprints {})",
                    if ko.footprints {
                        "allowed"
                    } else {
                        "not_allowed"
                    }
                ),
            );
            self.indent_level -= 1;
            self.line(out, ")");
        }

        // fill — 热参数与 island 模式一并带出, 缺省值对齐 KiCad 写盘习惯
        let island_tail = if zone.island_removal_mode == 2 {
            format!(" (island_area {})", Self::fmt_num(zone.island_area))
        } else {
            String::new()
        };
        self.line(
            out,
            &format!(
                "(fill {} (thermal_gap {}) (thermal_bridge_width {}) (island_removal_mode {}){})",
                if zone.fill { "yes" } else { "no" },
                Self::fmt_num(zone.thermal_gap),
                Self::fmt_num(zone.thermal_bridge_width),
                zone.island_removal_mode,
                island_tail
            ),
        );

        // polygon outline
        if !zone.outline.is_empty() {
            self.line(out, "(polygon");
            self.indent_level += 1;
            self.line(out, "(pts");
            self.indent_level += 1;
            for &(px, py) in &zone.outline {
                self.line(out, &Self::fmt_xy(px, py));
            }
            self.indent_level -= 1;
            self.line(out, ")");
            self.indent_level -= 1;
            self.line(out, ")");
        }

        // filled_polygon sections
        for fpoly in &zone.filled_polygons {
            if !fpoly.points.is_empty() {
                self.line(out, "(filled_polygon");
                self.indent_level += 1;
                self.line(out, &format!("(layer \"{}\")", fpoly.layer));
                self.line(out, "(pts");
                self.indent_level += 1;
                for &(px, py) in &fpoly.points {
                    self.line(out, &Self::fmt_xy(px, py));
                }
                self.indent_level -= 1;
                self.line(out, ")");
                // KiCad 10 filled_polygon has no (uuid ..) child — emitting one
                // makes the whole board unparseable ('expected )')

                self.indent_level -= 1;
                self.line(out, ")");
            }
        }
        self.indent_level -= 1;
        self.line(out, ")");
    }

    // ── Board graphics ───────────────────────────────────────────────────

    fn write_board_graphic(&mut self, out: &mut String, gr: &BoardGraphic) {
        match &gr.kind {
            BoardGraphicKind::Line { start, end } => {
                self.line(out, "(gr_line");
                self.indent_level += 1;
                let (sx, sy) = *start;
                let (ex, ey) = *end;
                self.line(
                    out,
                    &format!("(start {} {})", Self::fmt_num(sx), Self::fmt_num(sy)),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
                );
                self.line(
                    out,
                    &format!(
                        "(stroke (width {}) (type default))",
                        Self::fmt_num(gr.stroke_width)
                    ),
                );
                self.line(out, &format!("(layer \"{}\")", gr.layer));
                self.maybe_uuid(out);
                self.indent_level -= 1;
                self.line(out, ")");
            }
            BoardGraphicKind::Circle { center, end } => {
                self.line(out, "(gr_circle");
                self.indent_level += 1;
                let (cx, cy) = *center;
                let (ex, ey) = *end;
                self.line(
                    out,
                    &format!("(center {} {})", Self::fmt_num(cx), Self::fmt_num(cy)),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
                );
                self.line(
                    out,
                    &format!(
                        "(stroke (width {}) (type default))",
                        Self::fmt_num(gr.stroke_width)
                    ),
                );
                self.line(out, &format!("(layer \"{}\")", gr.layer));
                self.maybe_uuid(out);
                self.indent_level -= 1;
                self.line(out, ")");
            }
            BoardGraphicKind::Text {
                text,
                position,
                font_size,
            } => {
                self.line(out, &format!("(gr_text \"{}\"", Self::esc(text)));
                self.indent_level += 1;
                let (x, y, r) = *position;
                self.line(out, &Self::fmt_at(x, y, r));
                self.line(out, &format!("(layer \"{}\")", gr.layer));
                self.maybe_uuid(out);
                self.line(
                    out,
                    &format!(
                        "(effects (font (size {} {})))",
                        Self::fmt_num(*font_size),
                        Self::fmt_num(*font_size)
                    ),
                );
                self.indent_level -= 1;
                self.line(out, ")");
            }
            BoardGraphicKind::Arc { start, mid, end } => {
                self.line(out, "(gr_arc");
                self.indent_level += 1;
                let (sx, sy) = *start;
                let (mx, my) = *mid;
                let (ex, ey) = *end;
                self.line(
                    out,
                    &format!("(start {} {})", Self::fmt_num(sx), Self::fmt_num(sy)),
                );
                self.line(
                    out,
                    &format!("(mid {} {})", Self::fmt_num(mx), Self::fmt_num(my)),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
                );
                self.line(
                    out,
                    &format!(
                        "(stroke (width {}) (type default))",
                        Self::fmt_num(gr.stroke_width)
                    ),
                );
                self.line(out, &format!("(layer \"{}\")", gr.layer));
                self.maybe_uuid(out);
                self.indent_level -= 1;
                self.line(out, ")");
            }
            BoardGraphicKind::Poly { points } => {
                self.line(out, "(gr_poly");
                self.indent_level += 1;
                self.line(out, "(pts");
                self.indent_level += 1;
                for &(px, py) in points {
                    self.line(out, &Self::fmt_xy(px, py));
                }
                self.indent_level -= 1;
                self.line(out, ")");
                self.line(
                    out,
                    &format!(
                        "(stroke (width {}) (type default))",
                        Self::fmt_num(gr.stroke_width)
                    ),
                );
                self.line(out, &format!("(layer \"{}\")", gr.layer));
                if gr.fill {
                    self.line(out, "(fill solid)");
                }
                self.maybe_uuid(out);
                self.indent_level -= 1;
                self.line(out, ")");
            }
            BoardGraphicKind::Rect { start, end } => {
                self.line(out, "(gr_rect");
                self.indent_level += 1;
                let (sx, sy) = *start;
                let (ex, ey) = *end;
                self.line(
                    out,
                    &format!("(start {} {})", Self::fmt_num(sx), Self::fmt_num(sy)),
                );
                self.line(
                    out,
                    &format!("(end {} {})", Self::fmt_num(ex), Self::fmt_num(ey)),
                );
                self.line(
                    out,
                    &format!(
                        "(stroke (width {}) (type default))",
                        Self::fmt_num(gr.stroke_width)
                    ),
                );
                self.line(out, &format!("(layer \"{}\")", gr.layer));
                if gr.fill {
                    self.line(out, "(fill solid)");
                }
                self.maybe_uuid(out);
                self.indent_level -= 1;
                self.line(out, ")");
            }
        }
    }
}

impl Default for BoardSexprGenerator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_board() {
        let board = Board::new();
        // 华秋方言: 不写 nets 声明块
        let hq = BoardSexprConfig {
            dialect: crate::dialect::VendorDialect::Huaqiu,
            ..Default::default()
        };
        let mut gen = BoardSexprGenerator::with_config(hq);
        let output = gen.generate(&board).unwrap();

        assert!(output.starts_with("(kicad_pcb"));
        assert!(output.contains("(version 20240108)"));
        assert!(output.contains("(generator \"component-db\")"));
        assert!(output.contains("(layers"));
        assert!(output.contains("\"F.Cu\""));
        assert!(output.contains("\"B.Cu\""));
        assert!(!output.contains("(net 0"));
        assert!(output.ends_with(")\n"));
    }

    #[test]
    fn test_board_with_net() {
        let mut board = Board::new();
        let net_id = board.add_net("+5V");
        assert_eq!(net_id, 1);

        // 华秋方言: net 名内联在元素上, 无元素的 net 不产出声明行
        let hq = BoardSexprConfig {
            dialect: crate::dialect::VendorDialect::Huaqiu,
            ..Default::default()
        };
        let mut gen = BoardSexprGenerator::with_config(hq);
        let output = gen.generate(&board).unwrap();
        assert!(!output.contains("(net "));

        // Official 方言(默认): 无元素也写声明表
        let mut gen = BoardSexprGenerator::new();
        let output = gen.generate(&board).unwrap();
        assert!(
            output.contains("(net 1 \"+5V\")"),
            "official must emit the net table"
        );
    }

    #[test]
    fn test_board_with_footprint() {
        let mut board = Board::new();
        let net_id = board.find_or_add_net("GND");

        let mut fp = Footprint::new("Package_SO:SOP-8_3.9x4.9mm_P1.27mm", "U1", "CH340G");
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::Smd,
            shape: PadShape::RoundRect,
            position: (0.0, 0.0, 0.0),
            size: (0.6, 1.2),
            layers: vec!["F.Cu".into(), "F.Paste".into(), "F.Mask".into()],
            drill: None,
            net: Some(net_id),
            net_name: None,
            pin_function: Some("TXD".into()),
            pin_type: Some("output".into()),
            roundrect_rratio: Some(0.05),
            solder_mask_margin: None,
            thermal_bridge_width: None,
            thermal_bridge_angle: None,
            thermal_gap: None,
            clearance: None,
            zone_connect: None,
            remove_unused_layers: None,
            options: None,
            primitives: Vec::new(),
        });
        board.footprints.push(fp);

        let hq = BoardSexprConfig {
            dialect: crate::dialect::VendorDialect::Huaqiu,
            ..Default::default()
        };
        let mut gen = BoardSexprGenerator::with_config(hq);
        let output = gen.generate(&board).unwrap();

        assert!(output.contains("(footprint \"Package_SO:SOP-8_3.9x4.9mm_P1.27mm\""));
        assert!(output.contains("(pad \"1\" smd roundrect"));
        assert!(output.contains("(net \"GND\")"));
    }

    #[test]
    fn test_board_with_segment() {
        let mut board = Board::new();
        let net_id = board.add_net("+3V3");

        board.segments.push(Segment {
            start: (10.0, 20.0),
            end: (30.0, 20.0),
            width: 0.25,
            layer: "F.Cu".into(),
            net: net_id,
        });

        let mut gen = BoardSexprGenerator::new();
        let output = gen.generate(&board).unwrap();

        assert!(output.contains("(segment"));
        assert!(output.contains("(start 10 20)"));
        assert!(output.contains("(end 30 20)"));
        assert!(output.contains("(width 0.25)"));
    }

    #[test]
    fn test_board_with_zone() {
        let mut board = Board::new();
        let net_id = board.add_net("GND");

        board.zones.push(Zone {
            net: net_id,
            net_name: "GND".into(),
            layer: "F.Cu".into(),
            hatch_style: "full".into(),
            hatch_pitch: 0.508,
            pad_connect: "yes".into(),
            connect_pads_clearance: 0.3,
            min_thickness: 0.254,
            fill: true,
            thermal_gap: 0.508,
            thermal_bridge_width: 0.508,
            island_removal_mode: 0,
            island_area: 10.0,
            outline: vec![(0.0, 0.0), (50.0, 0.0), (50.0, 30.0), (0.0, 30.0)],
            filled_polygons: Vec::new(),
            keepout: None,
        });

        let hq = BoardSexprConfig {
            dialect: crate::dialect::VendorDialect::Huaqiu,
            ..Default::default()
        };
        let mut gen = BoardSexprGenerator::with_config(hq);
        let output = gen.generate(&board).unwrap();

        assert!(output.contains("(zone"));
        assert!(output.contains("(net \"GND\")"));
        assert!(output.contains("(polygon"));
        assert!(output.contains("(connect_pads yes (clearance 0.3))"));
        assert!(output.contains(
            "(fill yes (thermal_gap 0.508) (thermal_bridge_width 0.508) (island_removal_mode 0))"
        ));
    }

    #[test]
    fn test_pad_absolute_position() {
        let mut fp = Footprint::new("Test:FP", "U1", "IC");
        fp.position = (10.0, 20.0, 0.0);
        fp.pads.push(Pad {
            number: "1".into(),
            pad_type: PadType::Smd,
            shape: PadShape::Rect,
            position: (2.0, 3.0, 0.0),
            size: (1.0, 1.0),
            layers: vec!["F.Cu".into()],
            drill: None,
            net: None,
            net_name: None,
            pin_function: None,
            pin_type: None,
            roundrect_rratio: None,
            solder_mask_margin: None,
            thermal_bridge_width: None,
            thermal_bridge_angle: None,
            thermal_gap: None,
            clearance: None,
            zone_connect: None,
            remove_unused_layers: None,
            options: None,
            primitives: Vec::new(),
        });

        let (x, y) = fp.pad_absolute_pos("1").unwrap();
        assert!((x - 12.0).abs() < 0.001);
        assert!((y - 23.0).abs() < 0.001);
    }

    #[test]
    fn test_pad_custom_primitives_roundtrip() {
        let src = r#"(kicad_pcb (version 20241229) (generator "test")
	(net 0 "")
	(net 1 "GND")
	(footprint "Test:F" (layer "F.Cu") (at 0 0)
		(pad "1" smd custom
			(at 0 0) (size 1.475 0.9)
			(layers "F.Cu" "F.Mask" "F.Paste")
			(net 1 "GND")
			(zone_connect 2)
			(thermal_bridge_angle 45)
			(options
				(clearance outline)
				(anchor rect)
			)
			(primitives
				(gr_poly
					(pts (xy 3.8625 0.8665) (xy 0.7375 0.8665) (xy 0.7375 -0.8665) (xy 3.8625 -0.8665))
					(width 0)
					(fill yes)
				)
			)
		)
		(pad "2" thru_hole circle
			(at 3 0) (size 1.6 1.6) (drill 0.8)
			(layers "*.Cu" "*.Mask")
			(remove_unused_layers no)
			(net 1 "GND")
		)
	)
)"#;
        let board = crate::parse_board(src).unwrap();
        let out = BoardSexprGenerator::new().generate(&board).unwrap();
        assert!(out.contains("(options"), "options lost:\n{}", out);
        assert!(
            out.contains("(clearance outline)"),
            "options.clearance lost:\n{}",
            out
        );
        assert!(
            out.contains("(anchor rect)"),
            "options.anchor lost:\n{}",
            out
        );
        assert!(out.contains("(primitives"), "primitives lost:\n{}", out);
        assert!(out.contains("(gr_poly"), "gr_poly lost:\n{}", out);
        assert!(
            out.contains("(xy 3.8625 0.8665)"),
            "poly points lost:\n{}",
            out
        );
        assert!(
            out.contains("(remove_unused_layers no)"),
            "remove_unused_layers lost:\n{}",
            out
        );

        // 二次往返：新字段必须稳定（不再丢失、不变形）
        let board2 = crate::parse_board(&out).unwrap();
        let out2 = BoardSexprGenerator::new().generate(&board2).unwrap();
        for key in [
            "(options",
            "(clearance outline)",
            "(anchor rect)",
            "(gr_poly",
            "(remove_unused_layers no)",
        ] {
            assert!(out2.contains(key), "second pass lost {}:\n{}", key, out2);
        }
    }
}
