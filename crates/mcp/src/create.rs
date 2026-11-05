//! `drawing_create`: one call makes an editable DXF or DWG drawing from structured JSON.
//!
//! The drawing is built only from CADCraft's own commands (`new`, `layer.new`, `line`, `pline`,
//! `circle`, `arc`, `text`, `mtext`, `dim*`, `hatch`, `block`, `insert`, `dimstyle`, `saveas`),
//! never flattened to an image. Input is validated and capped before anything is drawn; an entity
//! the engine refuses is left out and reported in `dropped`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use serde_json::{Map, Value, json};

use crate::backend::Backend;
use crate::delivery::{MAX_ATTACH, mime_of, resource};

pub const MAX_ENTITIES: usize = 5000;
pub const MAX_BLOCKS: usize = 100;
pub const MAX_BLOCK_NESTING: usize = 8;
pub const MAX_LAYERS: usize = 256;
pub const MAX_POINTS: usize = 4096;
pub const MAX_TEXT: usize = 4000;
pub const MAX_TOTAL_TEXT: usize = 200_000;
/// Coordinates beyond this (in drawing units) are refused: finite, bounded extents.
pub const MAX_COORD: f64 = 1.0e9;
const MAX_NAME: usize = 255;

/// The input schema (also the tool's `inputSchema`).
pub fn schema() -> Value {
    let point = json!({"type": "array", "items": {"type": "number"}, "minItems": 2, "maxItems": 2});
    let entity = json!({"type": "object", "required": ["type"], "properties": {
        "type": {"type": "string", "enum": ["line", "polyline", "rectangle", "circle", "arc", "text", "mtext", "dimension", "hatch", "insert"]},
        "layer": {"type": "string"},
        "points": {"type": "array", "items": point, "description": "line, polyline, hatch boundary"},
        "closed": {"type": "boolean"},
        "width": {"type": "number", "description": "polyline width; mtext column width"},
        "corner1": point, "corner2": point,
        "center": point, "radius": {"type": "number"},
        "start": {"type": "number", "description": "arc start angle, degrees"}, "end": {"type": "number", "description": "arc end angle, degrees"},
        "at": point, "text": {"type": "string"}, "height": {"type": "number"}, "rotation": {"type": "number"},
        "justify": {"type": "string", "description": "L C R M TL TC TR ML MC MR BL BC BR"},
        "kind": {"type": "string", "enum": ["linear", "aligned", "radius", "diameter"], "description": "dimension kind (default linear)"},
        "p1": point, "p2": point, "point": point,
        "pattern": {"type": "string", "description": "hatch pattern: ANSI31 (default), SOLID, ANSI32, ANSI37, AR-CONC…"},
        "scale": {"type": "number"}, "angle": {"type": "number"},
        "name": {"type": "string", "description": "insert: block name"},
        "attribs": {"type": "object"}
    }});
    json!({"type": "object", "required": ["path"], "properties": {
        "path": {"type": "string", "description": "Output file, .dxf (recommended) or .dwg; relative to the working folder. An existing file is never replaced: the drawing gets an unused name beside it."},
        "units": {"type": "string", "enum": ["mm", "cm", "m", "in", "ft"], "description": "Drawing units (default mm)"},
        "style": {"type": "string", "enum": ["iso", "architectural"], "description": "Annotation style: iso (closed arrows, default) or architectural (tick marks)"},
        "scale": {"type": "number", "description": "Plot scale denominator (50 = 1:50) for text and dimension sizes; default: chosen from the extents to fit an A3 sheet"},
        "version": {"type": "integer", "description": "DXF/DWG version: 2000, 2004, 2007, 2010, 2013 or 2018 (default)"},
        "layers": {"type": "array", "items": {"type": "object", "required": ["name"], "properties": {
            "name": {"type": "string"},
            "color": {"description": "ACI number 1-255, a name (red, yellow…) or #rrggbb"},
            "linetype": {"type": "string", "description": "CONTINUOUS, DASHED, HIDDEN, CENTER, PHANTOM, DOT, DASHDOT, BORDER…"},
            "lineweight": {"type": "number", "description": "mm, e.g. 0.25, 0.35, 0.5"}
        }}},
        "blocks": {"type": "array", "items": {"type": "object", "required": ["name", "entities"], "properties": {
            "name": {"type": "string"}, "base": point, "entities": {"type": "array", "items": entity}
        }}},
        "entities": {"type": "array", "items": entity}
    }})
}

struct Units {
    insunits: u8,
    mm: f64,
    metric: bool,
}

fn units(u: &str) -> Result<Units, String> {
    let (insunits, mm, metric) = match u {
        "mm" => (4, 1.0, true),
        "cm" => (5, 10.0, true),
        "m" => (6, 1000.0, true),
        "in" => (1, 25.4, false),
        "ft" => (2, 304.8, false),
        _ => return Err(format!("`units` must be mm, cm, m, in or ft, not `{u}`")),
    };
    Ok(Units { insunits, mm, metric })
}

/// A finite point within [`MAX_COORD`].
fn point(v: &Value, what: &str) -> Result<[f64; 2], String> {
    let a = v.as_array().filter(|a| a.len() == 2).ok_or_else(|| format!("{what} must be [x, y]"))?;
    let mut p = [0.0; 2];
    for (i, c) in a.iter().enumerate() {
        let f = c
            .as_f64()
            .filter(|f| f.is_finite() && f.abs() <= MAX_COORD)
            .ok_or_else(|| format!("{what}: coordinates must be finite numbers within ±{MAX_COORD:e}"))?;
        if let Some(slot) = p.get_mut(i) {
            *slot = f;
        }
    }
    Ok(p)
}

fn num(e: &Map<String, Value>, k: &str, what: &str) -> Result<Option<f64>, String> {
    match e.get(k) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => {
            v.as_f64().filter(|f| f.is_finite() && f.abs() <= MAX_COORD).map(Some).ok_or_else(|| format!("{what}: `{k}` must be a finite number"))
        }
    }
}

fn positive(e: &Map<String, Value>, k: &str, what: &str) -> Result<Option<f64>, String> {
    match num(e, k, what)? {
        Some(f) if f <= 0.0 => Err(format!("{what}: `{k}` must be greater than 0")),
        other => Ok(other),
    }
}

fn need_point(e: &Map<String, Value>, k: &str, what: &str) -> Result<[f64; 2], String> {
    point(e.get(k).ok_or_else(|| format!("{what} needs `{k}`"))?, &format!("{what} `{k}`"))
}

/// A table name (layer, block): 1–255 characters without the ones DXF refuses.
fn name(v: &str, what: &str) -> Result<String, String> {
    let t = v.trim();
    if t.is_empty() || t.chars().count() > MAX_NAME || t.chars().any(|c| c.is_control() || "<>/\\\":;?*|,=`".contains(c)) {
        return Err(format!("{what} `{v}` is not a valid name (1-{MAX_NAME} characters, none of <>/\\\":;?*|,=`)"));
    }
    Ok(t.to_string())
}

/// What one entity becomes: a layer and the commands that draw it.
struct Planned {
    kind: String,
    layer: String,
    steps: Vec<(String, Value)>,
    /// A hatch: its boundary is drawn first, then hatched (`hatch` with the boundary's handle).
    hatch: Option<Value>,
}

struct Plan {
    units: Units,
    annotation: f64,
    text_height: f64,
    texts: usize,
    points: Vec<[f64; 2]>,
}

impl Plan {
    fn see(&mut self, p: [f64; 2]) -> Value {
        self.points.push(p);
        json!(p)
    }

    fn text(&mut self, s: &str, what: &str) -> Result<String, String> {
        let n = s.chars().count();
        if n == 0 || n > MAX_TEXT {
            return Err(format!("{what}: text must be 1-{MAX_TEXT} characters"));
        }
        self.texts = self.texts.saturating_add(n);
        if self.texts > MAX_TOTAL_TEXT {
            return Err(format!("more than {MAX_TOTAL_TEXT} characters of text in all"));
        }
        Ok(s.chars().filter(|c| !c.is_control() || *c == '\n').collect())
    }

    fn entity(&mut self, v: &Value, what: &str, blocks: &BTreeSet<String>) -> Result<Planned, String> {
        let e = v.as_object().ok_or_else(|| format!("{what} must be an object"))?;
        let kind = e.get("type").and_then(Value::as_str).ok_or_else(|| format!("{what} needs a `type`"))?.to_string();
        let what = format!("{what} ({kind})");
        let w = what.as_str();
        let layer = match e.get("layer").and_then(Value::as_str) {
            Some(l) => name(l, "layer")?,
            None => match kind.as_str() {
                "text" | "mtext" => "Text".into(),
                "dimension" => "Dimensions".into(),
                "hatch" => "Hatch".into(),
                _ => "0".into(),
            },
        };
        let pts = |plan: &mut Plan, min: usize| -> Result<Vec<Value>, String> {
            let a = e.get("points").and_then(Value::as_array).ok_or_else(|| format!("{w} needs `points`"))?;
            if a.len() < min || a.len() > MAX_POINTS {
                return Err(format!("{w}: {min}-{MAX_POINTS} points"));
            }
            a.iter().map(|p| point(p, w).map(|p| plan.see(p))).collect()
        };
        let closed = e.get("closed").and_then(Value::as_bool).unwrap_or(false);
        let mut steps = vec![];
        let mut hatch = None;
        match kind.as_str() {
            "line" => steps.push(("line".into(), json!({"points": pts(self, 2)?, "closed": closed}))),
            "polyline" => {
                let mut p = json!({"vertices": pts(self, 2)?, "closed": closed});
                if let Some(width) = num(e, "width", w)? {
                    p["width"] = json!(width.abs());
                }
                steps.push(("pline".into(), p));
            }
            "rectangle" => {
                let (a, b) = (need_point(e, "corner1", w)?, need_point(e, "corner2", w)?);
                let v = [[a[0], a[1]], [b[0], a[1]], [b[0], b[1]], [a[0], b[1]]].map(|p| self.see(p));
                steps.push(("pline".into(), json!({"vertices": v, "closed": true})));
            }
            "circle" => {
                let c = need_point(e, "center", w)?;
                let r = positive(e, "radius", w)?.ok_or_else(|| format!("{w} needs `radius`"))?;
                self.see([c[0] - r, c[1] - r]);
                self.see([c[0] + r, c[1] + r]);
                steps.push(("circle".into(), json!({"center": c, "radius": r})));
            }
            "arc" => {
                let c = need_point(e, "center", w)?;
                let r = positive(e, "radius", w)?.ok_or_else(|| format!("{w} needs `radius`"))?;
                let (s, en) =
                    (num(e, "start", w)?.ok_or_else(|| format!("{w} needs `start`"))?, num(e, "end", w)?.ok_or_else(|| format!("{w} needs `end`"))?);
                self.see([c[0] - r, c[1] - r]);
                self.see([c[0] + r, c[1] + r]);
                steps.push(("arc".into(), json!({"center": c, "radius": r, "start": s, "end": en})));
            }
            "text" | "mtext" => {
                let at = need_point(e, "at", w)?;
                let raw = e.get("text").and_then(Value::as_str).ok_or_else(|| format!("{w} needs `text`"))?;
                let text = self.text(raw, w)?;
                let text = if kind == "text" { text.replace('\n', " ") } else { text };
                let mut p = json!({"at": self.see(at), "text": text, "height": positive(e, "height", w)?.unwrap_or(self.text_height)});
                if let Some(r) = num(e, "rotation", w)? {
                    p["rotation"] = json!(r);
                }
                if let Some(j) = e.get("justify").and_then(Value::as_str) {
                    const J: [&str; 13] = ["L", "C", "R", "M", "TL", "TC", "TR", "ML", "MC", "MR", "BL", "BC", "BR"];
                    let j = j.to_ascii_uppercase();
                    if !J.contains(&j.as_str()) {
                        return Err(format!("{w}: `justify` must be one of {}", J.join(" ")));
                    }
                    p["justify"] = json!(j);
                }
                if kind == "mtext"
                    && let Some(width) = positive(e, "width", w)?
                {
                    p["width"] = json!(width);
                }
                steps.push((kind.clone(), p));
            }
            "dimension" => {
                let dk = e.get("kind").and_then(Value::as_str).unwrap_or("linear");
                let mut p = match dk {
                    "linear" | "aligned" => {
                        let (a, b, at) = (need_point(e, "p1", w)?, need_point(e, "p2", w)?, need_point(e, "at", w)?);
                        json!({"p1": self.see(a), "p2": self.see(b), "at": self.see(at)})
                    }
                    "radius" | "diameter" => {
                        let (c, pt) = (need_point(e, "center", w)?, need_point(e, "point", w)?);
                        json!({"center": self.see(c), "point": self.see(pt)})
                    }
                    other => return Err(format!("{w}: `kind` must be linear, aligned, radius or diameter, not `{other}`")),
                };
                if let Some(t) = e.get("text").and_then(Value::as_str) {
                    p["text"] = json!(self.text(t, w)?);
                }
                let id = match dk {
                    "aligned" => "dimaligned",
                    "radius" => "dimradius",
                    "diameter" => "dimdiameter",
                    _ => "dimlinear",
                };
                steps.push((id.into(), p));
            }
            "hatch" => {
                let boundary = pts(self, 3)?;
                steps.push(("pline".into(), json!({"vertices": boundary, "closed": true})));
                let pattern = e.get("pattern").and_then(Value::as_str).unwrap_or("ANSI31");
                if pattern.is_empty() || pattern.len() > 64 || !pattern.chars().all(|c| c.is_ascii_alphanumeric() || "-_".contains(c)) {
                    return Err(format!("{w}: `pattern` must be a pattern name such as ANSI31 or SOLID"));
                }
                let mut h = json!({"pattern": pattern.to_ascii_uppercase()});
                if !pattern.eq_ignore_ascii_case("SOLID") {
                    // Pattern spacing follows the plot scale, so the lines print about as far apart
                    // at 1:100 as at 1:1 (the patterns are defined in millimetres).
                    let auto = self.annotation / self.units.mm;
                    h["scale"] = json!(positive(e, "scale", w)?.unwrap_or(auto));
                    if let Some(a) = num(e, "angle", w)? {
                        h["angle"] = json!(a);
                    }
                }
                hatch = Some(h);
            }
            "insert" => {
                let block = name(e.get("name").and_then(Value::as_str).ok_or_else(|| format!("{w} needs the block `name`"))?, "block")?;
                if !blocks.contains(&block) {
                    return Err(format!("{w}: block `{block}` is not defined before it is used"));
                }
                let at = need_point(e, "at", w)?;
                let mut p = json!({"name": block, "at": self.see(at)});
                if let Some(s) = num(e, "scale", w)? {
                    if s == 0.0 {
                        return Err(format!("{w}: `scale` can't be 0"));
                    }
                    p["scale"] = json!(s);
                }
                if let Some(r) = num(e, "rotation", w)? {
                    p["rotation"] = json!(r);
                }
                if let Some(a) = e.get("attribs").and_then(Value::as_object) {
                    let mut out = Map::new();
                    for (k, v) in a.iter().take(64) {
                        out.insert(name(k, "attribute tag")?, json!(self.text(v.as_str().unwrap_or_default(), w)?));
                    }
                    p["attribs"] = Value::Object(out);
                }
                steps.push(("insert".into(), p));
            }
            other => return Err(format!("{w}: unknown type `{other}`")),
        }
        Ok(Planned { kind, layer, steps, hatch })
    }
}

/// The smallest usual plot scale (1:n) at which `extent_mm` fits an A3 sheet's 380 mm.
fn plot_scale(extent_mm: f64) -> f64 {
    const SCALES: [f64; 16] = [1.0, 2.0, 5.0, 10.0, 20.0, 25.0, 50.0, 100.0, 200.0, 250.0, 500.0, 1000.0, 2000.0, 2500.0, 5000.0, 10000.0];
    SCALES.into_iter().find(|s| extent_mm / s <= 380.0).unwrap_or(10000.0)
}

fn handles_of(v: &Value) -> Vec<String> {
    let mut h: Vec<String> = v["handles"].as_array().into_iter().flatten().filter_map(Value::as_str).map(String::from).collect();
    if let Some(one) = v["handle"].as_str()
        && !h.iter().any(|x| x == one)
    {
        h.push(one.to_string());
    }
    h
}

fn layer_props(l: &Map<String, Value>) -> Result<Value, String> {
    let mut p = json!({"name": name(l.get("name").and_then(Value::as_str).unwrap_or_default(), "layer")?});
    match l.get("color") {
        None | Some(Value::Null) => {}
        Some(Value::Number(n)) => match n.as_u64().filter(|c| (1..=255).contains(c)) {
            Some(c) => p["color"] = json!(c),
            None => return Err("layer `color` numbers are ACI colours 1-255".into()),
        },
        Some(Value::String(s)) if !s.is_empty() && s.len() <= 32 && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '#') => p["color"] = json!(s),
        Some(_) => return Err("layer `color` must be 1-255, a colour name or #rrggbb".into()),
    }
    if let Some(lt) = l.get("linetype").and_then(Value::as_str) {
        p["linetype"] = json!(name(lt, "linetype")?.to_ascii_uppercase());
    }
    if let Some(w) = l.get("lineweight") {
        let w = w.as_f64().filter(|w| (0.0..=2.11).contains(w)).ok_or("layer `lineweight` must be 0-2.11 mm")?;
        p["lineweight"] = json!(w);
    }
    Ok(p)
}

/// The colour new layers get when the input names none, by their role.
fn default_color(layer: &str) -> Value {
    match layer {
        "Text" => json!(7),
        "Dimensions" => json!(4),
        "Hatch" => json!(8),
        _ => json!(7),
    }
}

/// Run `drawing_create`. `Err` is an `isError` result (bad input or a failed save).
pub fn create(b: &mut dyn Backend, a: &Value) -> Result<Value, String> {
    let path = a.get("path").and_then(Value::as_str).filter(|p| !p.trim().is_empty()).ok_or("drawing_create needs a `path`")?;
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or_default().to_ascii_lowercase();
    if ext != "dxf" && ext != "dwg" {
        return Err("drawing_create writes .dxf or .dwg files".into());
    }
    let units = units(a.get("units").and_then(Value::as_str).unwrap_or("mm"))?;
    let architectural = match a.get("style").and_then(Value::as_str).unwrap_or("iso") {
        "iso" => false,
        "architectural" => true,
        other => return Err(format!("`style` must be iso or architectural, not `{other}`")),
    };
    let version = match a.get("version") {
        None | Some(Value::Null) => None,
        Some(v) => Some(
            v.as_u64()
                .filter(|v| [2000, 2004, 2007, 2010, 2013, 2018].contains(v))
                .ok_or("`version` must be 2000, 2004, 2007, 2010, 2013 or 2018")?,
        ),
    };
    let list = |k: &str, max: usize| -> Result<Vec<Value>, String> {
        match a.get(k) {
            None | Some(Value::Null) => Ok(vec![]),
            Some(Value::Array(v)) if v.len() <= max => Ok(v.clone()),
            Some(Value::Array(_)) => Err(format!("at most {max} {k}")),
            Some(_) => Err(format!("`{k}` must be an array")),
        }
    };
    let (layers_in, blocks_in, entities_in) = (list("layers", MAX_LAYERS)?, list("blocks", MAX_BLOCKS)?, list("entities", MAX_ENTITIES)?);

    // Plan everything first: nothing is drawn unless the whole input is valid.
    let mut plan = Plan { units, annotation: 1.0, text_height: 0.0, texts: 0, points: vec![] };
    let mut layers: BTreeMap<String, Value> = BTreeMap::new();
    for (i, l) in layers_in.iter().enumerate() {
        let props = layer_props(l.as_object().ok_or_else(|| format!("layers[{i}] must be an object"))?)?;
        let key = props["name"].as_str().unwrap_or_default().to_lowercase();
        if layers.insert(key, props).is_some() {
            return Err(format!("layers[{i}]: the layer is listed twice"));
        }
    }
    // Two passes: the first finds the extents (for the plot scale), the second plans with the
    // text height that scale gives. Sizes in the first pass don't matter.
    let plan_all = |plan: &mut Plan| -> Result<(Vec<(String, [f64; 2], Vec<Planned>)>, Vec<Planned>), String> {
        let mut defined: BTreeSet<String> = BTreeSet::new();
        let mut depth: BTreeMap<String, usize> = BTreeMap::new();
        let mut total = entities_in.len();
        let mut blocks = vec![];
        for (i, blk) in blocks_in.iter().enumerate() {
            let o = blk.as_object().ok_or_else(|| format!("blocks[{i}] must be an object"))?;
            let bname = name(o.get("name").and_then(Value::as_str).unwrap_or_default(), "block")?;
            if defined.contains(&bname) {
                return Err(format!("blocks[{i}]: block `{bname}` is defined twice"));
            }
            let base = match o.get("base") {
                Some(v) => point(v, &format!("blocks[{i}] base"))?,
                None => [0.0, 0.0],
            };
            let ents = o.get("entities").and_then(Value::as_array).ok_or_else(|| format!("blocks[{i}] needs `entities`"))?;
            total = total.saturating_add(ents.len());
            if total > MAX_ENTITIES {
                return Err(format!("more than {MAX_ENTITIES} entities in all (blocks included)"));
            }
            let mut d = 1;
            let mut planned = vec![];
            for (j, e) in ents.iter().enumerate() {
                let p = plan.entity(e, &format!("blocks[{i}].entities[{j}]"), &defined)?;
                if p.kind == "insert"
                    && let Some(inner) = p.steps.first().and_then(|s| s.1["name"].as_str())
                {
                    d = d.max(depth.get(inner).copied().unwrap_or(1).saturating_add(1));
                }
                planned.push(p);
            }
            if d > MAX_BLOCK_NESTING {
                return Err(format!("blocks[{i}]: blocks nest more than {MAX_BLOCK_NESTING} deep"));
            }
            depth.insert(bname.clone(), d);
            defined.insert(bname.clone());
            blocks.push((bname, base, planned));
        }
        let mut ents = vec![];
        for (i, e) in entities_in.iter().enumerate() {
            ents.push(plan.entity(e, &format!("entities[{i}]"), &defined)?);
        }
        Ok((blocks, ents))
    };
    plan_all(&mut plan)?;
    let (mut lo, mut hi) = ([f64::MAX; 2], [f64::MIN; 2]);
    for p in &plan.points {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let extent = if plan.points.is_empty() { 0.0 } else { (hi[0] - lo[0]).max(hi[1] - lo[1]) };
    plan.annotation = match a.get("scale") {
        None | Some(Value::Null) => plot_scale(extent * plan.units.mm),
        Some(v) => {
            v.as_f64().filter(|s| s.is_finite() && (0.001..=100_000.0).contains(s)).ok_or("`scale` must be a plot scale denominator, 0.001-100000")?
        }
    };
    // 2.5 mm text on paper (1/8" in imperial drawings), in drawing units at the plot scale.
    let paper_text = if plan.units.metric { 2.5 } else { 3.175 };
    plan.text_height = paper_text * plan.annotation / plan.units.mm;
    plan.points.clear();
    plan.texts = 0;
    let (blocks, entities) = plan_all(&mut plan)?;

    // Draw.
    let run = |b: &mut dyn Backend, id: &str, p: Value| b.call("engine.execute", json!({"command": id, "params": p}));
    run(b, "new", json!({"metric": plan.units.metric, "template": "."}))?;
    run(b, "units", json!({"insunits": plan.units.insunits}))?;
    // Paper millimetres in drawing units at the plot scale.
    let size = |mm: f64| mm * plan.annotation / plan.units.mm;
    let mut ds = json!({"name": "CADCraft", "current": true, "DIMSCALE": 1.0, "DIMTXT": plan.text_height, "DIMASZ": plan.text_height,
        "DIMEXO": size(0.625), "DIMEXE": size(1.25), "DIMGAP": size(0.625), "DIMTAD": 1, "DIMDEC": if plan.units.metric { 0 } else { 2 }});
    if architectural {
        ds["DIMTSZ"] = json!(size(1.5));
        // Feet and inches (10'-6") need inch drawing units.
        if plan.units.insunits == 1 {
            ds["DIMLUNIT"] = json!(4);
        }
    }
    let mut warnings: Vec<String> = vec![];
    if let Err(e) = run(b, "dimstyle", ds) {
        warnings.push(format!("dimension style: {e}"));
    }
    let mut loaded: BTreeSet<String> = ["CONTINUOUS", "BYLAYER", "BYBLOCK"].into_iter().map(String::from).collect();
    let mut made: BTreeSet<String> = BTreeSet::new();
    made.insert("0".into());
    let mut dropped: Vec<Value> = vec![];
    let mut ensure_layer = |b: &mut dyn Backend, layer: &str, dropped: &mut Vec<Value>| {
        let key = layer.to_lowercase();
        if made.contains(&key) {
            return;
        }
        let mut props = layers.get(&key).cloned().unwrap_or_else(|| json!({"name": layer, "color": default_color(layer)}));
        if let Some(lt) = props["linetype"].as_str().map(String::from)
            && !loaded.contains(&lt)
        {
            match run(b, "linetype", json!({"load": lt})) {
                Ok(_) => {
                    loaded.insert(lt);
                }
                Err(e) => {
                    dropped.push(json!({"layer": layer, "linetype": lt, "error": e}));
                    if let Some(o) = props.as_object_mut() {
                        o.remove("linetype");
                    }
                }
            }
        }
        if let Err(e) = run(b, "layer.new", props) {
            dropped.push(json!({"layer": layer, "error": e}));
        }
        made.insert(key);
    };
    for l in layers.values() {
        let n = l["name"].as_str().unwrap_or_default().to_string();
        ensure_layer(b, &n, &mut dropped);
    }
    let mut current = String::new();
    let mut created = 0usize;
    let mut draw = |b: &mut dyn Backend,
                    p: &Planned,
                    at: &str,
                    dropped: &mut Vec<Value>,
                    made_layers: &mut dyn FnMut(&mut dyn Backend, &str, &mut Vec<Value>)|
     -> Vec<String> {
        made_layers(b, &p.layer, dropped);
        if current != p.layer.to_lowercase() {
            let _ = run(b, "layer.current", json!({"name": p.layer}));
            current = p.layer.to_lowercase();
        }
        let mut handles = vec![];
        for (id, params) in &p.steps {
            match run(b, id, params.clone()) {
                Ok(v) => handles.extend(handles_of(&v)),
                Err(e) => {
                    dropped.push(json!({"entity": at, "type": p.kind, "error": e}));
                    return vec![];
                }
            }
        }
        if let Some(h) = &p.hatch {
            let mut h = h.clone();
            h["handles"] = json!(handles.clone());
            match run(b, "hatch", h) {
                Ok(v) => handles.extend(handles_of(&v)),
                Err(e) => dropped.push(json!({"entity": at, "type": "hatch", "error": e})),
            }
        }
        created += usize::from(!handles.is_empty());
        handles
    };
    let mut ensure = |b: &mut dyn Backend, l: &str, d: &mut Vec<Value>| ensure_layer(b, l, d);
    for (bname, base, ents) in &blocks {
        let mut handles = vec![];
        for (j, p) in ents.iter().enumerate() {
            handles.extend(draw(b, p, &format!("block {bname} entity {j}"), &mut dropped, &mut ensure));
        }
        if handles.is_empty() {
            dropped.push(json!({"block": bname, "error": "no entities could be drawn"}));
            continue;
        }
        if let Err(e) = run(b, "block", json!({"name": bname, "base": base, "handles": handles, "keep": "delete"})) {
            dropped.push(json!({"block": bname, "error": e}));
        }
    }
    for (i, p) in entities.iter().enumerate() {
        draw(b, p, &format!("entities[{i}]"), &mut dropped, &mut ensure);
    }
    let _ = run(b, "layer.current", json!({"name": "0"}));
    let _ = run(b, "zoom", json!({"mode": "extents"}));

    // Refuse an oversized result before anything is published.
    let mut bytes_params = json!({"format": ext});
    if let Some(v) = version {
        bytes_params["version"] = json!(v);
    }
    let measured = run(b, "document.bytes", bytes_params)?;
    let size_bytes = measured["bytes"].as_u64().unwrap_or(u64::MAX);
    if size_bytes > MAX_ATTACH {
        return Err(format!("the drawing would be {size_bytes} bytes, over the 32 MiB limit; nothing was saved"));
    }
    let target = cadcraft_engine::file_access::unused_name(Path::new(path))?;
    let target_s = target.to_string_lossy().into_owned();
    let mut save = json!({"path": target_s});
    if let Some(v) = version {
        save["version"] = json!(v);
    }
    let saved = run(b, "saveas", save)?;
    let (abs, data) = crate::delivery::read_bounded(&target_s)?;
    let file_name = abs.file_name().and_then(|n| n.to_str()).unwrap_or("drawing").to_string();
    let mime = mime_of(&abs)?;
    for key in ["notSaved", "warnings", "message"] {
        if let Some(v) = saved.get(key).filter(|v| !v.is_null()) {
            warnings.push(format!("{key}: {v}"));
        }
    }
    let summary = json!({
        "path": abs, "name": file_name, "format": ext, "bytes": data.len(), "mimeType": mime,
        "entities": created, "layers": made.len(), "blocks": blocks.len(),
        "units": a.get("units").and_then(Value::as_str).unwrap_or("mm"), "plotScale": format!("1:{}", plan.annotation), "textHeight": plan.text_height,
        "dropped": dropped, "warnings": warnings,
        "next": "The file is attached. To edit it later, open this path; save under a new name, then call file_attach.",
    });
    Ok(json!({"content": [
        {"type": "text", "text": serde_json::to_string_pretty(&summary).unwrap_or_default()},
        resource(&file_name, mime, &data),
    ]}))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plot_scale_fits_an_a3_sheet() {
        assert_eq!(plot_scale(0.0), 1.0);
        assert_eq!(plot_scale(300.0), 1.0);
        assert_eq!(plot_scale(12_000.0), 50.0);
        assert_eq!(plot_scale(1.0e12), 10000.0);
    }

    #[test]
    fn names_and_points_are_checked() {
        assert!(name("Walls", "layer").is_ok());
        for bad in ["", "a/b", "a*", &"x".repeat(256)] {
            assert!(name(bad, "layer").is_err(), "{bad}");
        }
        assert!(point(&json!([1, 2]), "p").is_ok());
        for bad in [json!([1]), json!([1, 2, 3]), json!(["a", 1]), json!([1e10, 0]), json!(null)] {
            assert!(point(&bad, "p").is_err(), "{bad}");
        }
    }
}
