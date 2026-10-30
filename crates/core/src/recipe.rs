//! Versioned edit-recipe model (docs/edit-recipe-spec-v1.md, task 00-011).
//!
//! A recipe is `{ spec, version, ops[] }`; ops apply top-to-bottom. Parsing
//! enforces schema-level validity (spec discriminator, supported version,
//! known op types with in-range params), skips unknown op types with a
//! warning per §2/§5 conformance, and fills documented param defaults.
//! Serialization is a pure round-trip: parse(to_json(r)) == r.

use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

/// Discriminator of an edit recipe packet (spec §1).
pub const RECIPE_SPEC: &str = "fotobase.recipe";
/// Spec shape version this model implements (spec §4).
pub const RECIPE_VERSION: u32 = 1;

/// Every v1 op type (spec §3 / docs/recipe-v1.schema.json `opType`).
pub const V1_OP_TYPES: &[&str] = &[
    "crop",
    "straighten",
    "auto_color",
    "auto_contrast",
    "fill_light",
    "temperature",
    "tint",
    "saturation",
    "brightness",
    "highlight_recovery",
    "shadow_recovery",
    "sepia",
    "warm_colors",
    "cool_colors",
    "red_eye",
    "text",
    "retouch",
    "grayscale",
    "bw",
    "sharpen",
    "undo_all",
];

/// A parsed, validated edit recipe.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    pub spec: String,
    pub version: u32,
    pub ops: Vec<Op>,
}

/// A single op. `params` is kept as raw JSON (validated per type) so unknown
/// future params survive a round-trip verbatim.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Op {
    #[serde(rename = "type")]
    pub op_type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub params: Option<Map<String, Value>>,
}

#[derive(Debug)]
pub struct RecipeError(pub String);

impl fmt::Display for RecipeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid recipe: {}", self.0)
    }
}

impl std::error::Error for RecipeError {}

fn err<T>(msg: impl Into<String>) -> Result<T, RecipeError> {
    Err(RecipeError(msg.into()))
}

/// Parse a recipe packet. Returns the recipe plus warnings for skipped
/// unknown ops / unknown params (spec §2: skip and warn, never fail).
pub fn parse(json: &str) -> Result<(Recipe, Vec<String>), RecipeError> {
    let value: Value =
        serde_json::from_str(json).map_err(|e| RecipeError(format!("not valid JSON: {e}")))?;
    parse_value(&value)
}

pub fn parse_value(value: &Value) -> Result<(Recipe, Vec<String>), RecipeError> {
    let obj =
        value.as_object().ok_or_else(|| RecipeError("recipe must be a JSON object".into()))?;
    if obj.get("spec").and_then(Value::as_str) != Some(RECIPE_SPEC) {
        return err("spec must be \"fotobase.recipe\"");
    }
    let version = match obj.get("version").and_then(Value::as_u64) {
        Some(v @ 1..) => {
            u32::try_from(v).map_err(|_| RecipeError("version out of range".into()))?
        }
        _ => return err("version must be an integer >= 1"),
    };
    if version > RECIPE_VERSION {
        return err(format!(
            "unsupported recipe version {version} (this reader understands up to {RECIPE_VERSION})"
        ));
    }
    let ops = obj
        .get("ops")
        .and_then(Value::as_array)
        .ok_or_else(|| RecipeError("ops must be an array".into()))?;

    let mut warnings = Vec::new();
    let mut parsed = Vec::with_capacity(ops.len());
    for (i, op) in ops.iter().enumerate() {
        let obj =
            op.as_object().ok_or_else(|| RecipeError(format!("ops[{i}] must be an object")))?;
        let op_type = obj
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| RecipeError(format!("ops[{i}].type must be a string")))?;
        if !V1_OP_TYPES.contains(&op_type) {
            warnings.push(format!("ops[{i}]: unknown op type {op_type:?} skipped"));
            continue;
        }
        let params = match obj.get("params") {
            None => None,
            Some(Value::Null) => None,
            Some(p @ Value::Object(_)) => {
                validate_params(op_type, p, i)?;
                Some(p.as_object().unwrap().clone())
            }
            Some(_) => return err(format!("ops[{i}].params must be an object")),
        };
        let extra = validate_extra_params(op_type, &params, i)?;
        warnings.extend(extra);
        parsed.push(Op { op_type: op_type.to_string(), params });
    }
    Ok((Recipe { spec: RECIPE_SPEC.into(), version, ops: parsed }, warnings))
}

/// One forward migration step: recipes of spec shape `from` are rewritten to
/// shape `from + 1` (spec §4). Each step is a pure function over the parsed
/// packet; the chain is applied in order on read.
type MigrationStep = fn(Recipe, &mut Vec<String>) -> Recipe;

/// Forward migrations, indexed so `MIGRATIONS[v - 1]` upgrades a v-`v`
/// recipe to v-`v+1`. Empty today: v1 is the current shape, so migration is
/// the identity — the seam is the point (spec §4: migrate forward on read,
/// write back migrated on next save).
const MIGRATIONS: &[MigrationStep] = &[];

/// Read path for stored recipes (TASK-04-002): parse, then apply every
/// migration up to the current spec version, transparently. A recipe written
/// by a newer reader is rejected, never down-migrated.
pub fn migrate(json: &str) -> Result<(Recipe, Vec<String>), RecipeError> {
    let (mut recipe, mut warnings) = parse(json)?;
    while recipe.version < RECIPE_VERSION {
        let step = MIGRATIONS[recipe.version as usize - 1];
        recipe = step(recipe, &mut warnings);
        recipe.version += 1;
    }
    Ok((recipe, warnings))
}

impl Recipe {
    /// Parse a recipe packet; warnings (unknown ops) are dropped.
    pub fn parse(json: &str) -> Result<(Recipe, Vec<String>), RecipeError> {
        parse(json)
    }
    /// Serialize back to a conforming packet (round-trip: parse(to_json(r)) == r).
    pub fn to_json(&self) -> String {
        serde_json::to_string(&json!({
            "spec": self.spec,
            "version": self.version,
            "ops": self.ops,
        }))
        .expect("recipe serializes")
    }
}

// ---------------------------------------------------------------------------
// Param validation (ranges from docs/edit-recipe-spec-v1.md §3)
// ---------------------------------------------------------------------------

fn num(params: &Map<String, Value>, key: &str) -> Option<f64> {
    params.get(key).and_then(Value::as_f64)
}

fn require_range(
    op_type: &str,
    params: &Map<String, Value>,
    key: &str,
    lo: f64,
    hi: f64,
    idx: usize,
) -> Result<(), RecipeError> {
    match num(params, key) {
        Some(v) if v.is_finite() && (lo..=hi).contains(&v) => Ok(()),
        Some(v) => err(format!("ops[{idx}] {op_type}.{key} = {v} out of range [{lo}, {hi}]")),
        None => Ok(()), // absent → default
    }
}

fn validate_params(op_type: &str, params: &Value, idx: usize) -> Result<(), RecipeError> {
    use crate::apply::parse_hex_color;
    let p = params.as_object().unwrap();
    let unit = |key: &str, lo: f64, hi: f64| require_range(op_type, p, key, lo, hi, idx);
    match op_type {
        "crop" => {
            unit("x", 0.0, 1.0)?;
            unit("y", 0.0, 1.0)?;
            let (w, h) = (num(p, "w").unwrap_or(1.0), num(p, "h").unwrap_or(1.0));
            if !(0.0..=1.0).contains(&w) || !(0.0..=1.0).contains(&h) || w <= 0.0 || h <= 0.0 {
                return err(format!("ops[{idx}] crop w/h must be in (0, 1]"));
            }
            if num(p, "x").is_some_and(|x| x + w > 1.0 + 1e-9)
                || num(p, "y").is_some_and(|y| y + h > 1.0 + 1e-9)
            {
                return err(format!("ops[{idx}] crop rect exceeds the image"));
            }
        }
        "straighten" => {
            unit("angle", -45.0, 45.0)?;
            if let Some(v) = p.get("auto_fill") {
                if !v.is_boolean() {
                    return err(format!("ops[{idx}] straighten.auto_fill must be a boolean"));
                }
            }
        }
        "fill_light" | "highlight_recovery" | "shadow_recovery" | "sepia" | "warm_colors"
        | "cool_colors" => unit("amount", 0.0, 1.0)?,
        "temperature" => unit("kelvin_shift", -1.0, 1.0)?,
        "tint" => unit("green_magenta", -1.0, 1.0)?,
        "saturation" | "brightness" => unit("amount", -1.0, 1.0)?,
        "sharpen" => unit("amount", 0.0, 1.0)?,
        "bw" => unit("amount", 0.0, 1.0)?,
        "grayscale" => {
            if let Some(v) = p.get("enabled") {
                if !v.is_boolean() {
                    return err(format!("ops[{idx}] grayscale.enabled must be a boolean"));
                }
            }
        }
        "red_eye" => {
            if let Some(v) = p.get("auto") {
                if !v.is_boolean() {
                    return err(format!("ops[{idx}] red_eye.auto must be a boolean"));
                }
            }
            // `regions` is required unless `auto` requests detection.
            let auto = p.get("auto").and_then(Value::as_bool).unwrap_or(false);
            let regions = p.get("regions").and_then(Value::as_array);
            if regions.is_none() && !auto {
                return err(format!(
                    "ops[{idx}] red_eye.regions must be an array (or set auto=true)"
                ));
            }
            if let Some(regions) = regions {
                for (j, r) in regions.iter().enumerate() {
                    let r = r.as_object().ok_or_else(|| {
                        RecipeError(format!("ops[{idx}] red_eye.regions[{j}] must be an object"))
                    })?;
                    for key in ["x", "y", "r"] {
                        let v = r.get(key).and_then(Value::as_f64).ok_or_else(|| {
                            RecipeError(format!(
                                "ops[{idx}] red_eye.regions[{j}].{key} must be a number"
                            ))
                        })?;
                        if !(0.0..=1.0).contains(&v) {
                            return err(format!(
                                "ops[{idx}] red_eye.regions[{j}].{key} = {v} out of range [0, 1]"
                            ));
                        }
                    }
                }
            }
        }
        "text" => {
            let text = p
                .get("text")
                .and_then(Value::as_str)
                .ok_or_else(|| RecipeError(format!("ops[{idx}] text.text must be a string")))?;
            if text.is_empty() {
                return err(format!("ops[{idx}] text.text must not be empty"));
            }
            unit("x", 0.0, 1.0)?;
            unit("y", 0.0, 1.0)?;
            unit("size", 0.0, 1.0)?;
            unit("rotation", -180.0, 180.0)?;
            if let Some(c) = p.get("color") {
                match c {
                    Value::String(hex) => {
                        if parse_hex_color(hex).is_none() {
                            return err(format!("ops[{idx}] text.color must be \"#rrggbb\""));
                        }
                    }
                    Value::Array(c) => {
                        if c.len() != 3 || c.iter().any(|v| !v.is_number()) {
                            return err(format!("ops[{idx}] text.color must be [r, g, b]"));
                        }
                    }
                    _ => {
                        return err(format!(
                            "ops[{idx}] text.color must be \"#rrggbb\" or [r, g, b]"
                        ))
                    }
                }
            }
        }
        "retouch" => {
            let strokes = p.get("strokes").and_then(Value::as_array).ok_or_else(|| {
                RecipeError(format!("ops[{idx}] retouch.strokes must be an array"))
            })?;
            for (j, s) in strokes.iter().enumerate() {
                let s = s.as_object().ok_or_else(|| {
                    RecipeError(format!("ops[{idx}] retouch.strokes[{j}] must be an object"))
                })?;
                let pts = s.get("points").and_then(Value::as_array).ok_or_else(|| {
                    RecipeError(format!("ops[{idx}] retouch.strokes[{j}].points must be an array"))
                })?;
                if pts.is_empty() {
                    return err(format!(
                        "ops[{idx}] retouch.strokes[{j}].points must not be empty"
                    ));
                }
                for (k, pt) in pts.iter().enumerate() {
                    let pt = pt.as_array().ok_or_else(|| {
                        RecipeError(format!(
                            "ops[{idx}] retouch.strokes[{j}].points[{k}] must be [x, y]"
                        ))
                    })?;
                    if pt.len() != 2 || pt.iter().any(|v| !v.is_number()) {
                        return err(format!(
                            "ops[{idx}] retouch.strokes[{j}].points[{k}] must be [x, y]"
                        ));
                    }
                    if pt.iter().any(|v| !(0.0..=1.0).contains(&v.as_f64().unwrap())) {
                        return err(format!(
                            "ops[{idx}] retouch.strokes[{j}].points[{k}] out of range [0, 1]"
                        ));
                    }
                }
                let radius = s.get("radius").and_then(Value::as_f64).ok_or_else(|| {
                    RecipeError(format!("ops[{idx}] retouch.strokes[{j}].radius must be a number"))
                })?;
                if !(0.0..=1.0).contains(&radius) {
                    return err(format!(
                        "ops[{idx}] retouch.strokes[{j}].radius out of range [0, 1]"
                    ));
                }
            }
        }
        "auto_color" | "auto_contrast" | "undo_all" => {}
        other => return err(format!("ops[{idx}] unknown op type {other:?}")),
    }
    Ok(())
}

/// Unknown params on a *known* op are preserved but warned about (spec §5:
/// writer emits documented params; reader must not lose foreign data).
fn validate_extra_params(
    op_type: &str,
    params: &Option<Map<String, Value>>,
    idx: usize,
) -> Result<Vec<String>, RecipeError> {
    let Some(params) = params else { return Ok(Vec::new()) };
    let documented: &[&str] = match op_type {
        "crop" => &["x", "y", "w", "h"],
        "straighten" => &["angle", "auto_fill"],
        "fill_light" | "highlight_recovery" | "shadow_recovery" | "sepia" | "warm_colors"
        | "cool_colors" | "saturation" | "brightness" | "sharpen" | "bw" => &["amount"],
        "temperature" => &["kelvin_shift"],
        "tint" => &["green_magenta"],
        "grayscale" => &["enabled"],
        "red_eye" => &["regions", "auto"],
        "text" => &["text", "x", "y", "size", "color", "font", "rotation"],
        "retouch" => &["strokes"],
        _ => &[],
    };
    let extra: Vec<&str> = params
        .keys()
        .filter(|k| !documented.contains(&k.as_str()) && *k != "algo")
        .map(String::as_str)
        .collect();
    if extra.is_empty() {
        Ok(Vec::new())
    } else {
        Ok(vec![format!("ops[{idx}]: {op_type} has undocumented params {extra:?} (preserved)")])
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn empty_recipe_round_trips() {
        let (r, w) = parse(r#"{"spec":"fotobase.recipe","version":1,"ops":[]}"#).unwrap();
        assert!(w.is_empty());
        assert!(r.ops.is_empty());
        assert_eq!(parse(&r.to_json()).unwrap().0, r);
    }

    #[test]
    fn algo_param_is_preserved_and_not_warned() {
        let (r, w) = parse(
            r#"{"spec":"fotobase.recipe","version":1,
                 "ops":[{"type":"sepia","params":{"amount":0.5,"algo":1}}]}"#,
        )
        .unwrap();
        assert!(w.is_empty());
        assert_eq!(r.ops[0].params.as_ref().unwrap()["algo"], json!(1));
        assert_eq!(parse(&r.to_json()).unwrap().0, r);
    }
}
