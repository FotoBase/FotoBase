//! Portable export of user data (TASK-03-023, PLAN §5 "Portability
//! guarantee"): albums, tags and edit recipes to JSON or CSV, at any time,
//! so no user data is held hostage in the catalog.
//!
//! - **JSON** is the full-fidelity, self-describing, versioned document
//!   (`docs/portable-export-v1.schema.json`); recipes are embedded as
//!   conforming `docs/recipe-v1.schema.json` packets.
//! - **CSV** writes one RFC 4180 table per picked section.
//!
//! Albums and tags come from the catalog mirror; recipes are read from the
//! `fb:Recipe` sidecar field (the catalog only flags their presence). The
//! export is read-only for sidecars and catalog, makes no network calls, and
//! never overwrites an earlier export.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use fotobase_metadata::Sidecar;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::sync::sidecar_path;
use crate::{civil_from_days, Catalog, CatalogError, Result};

/// Discriminator of the export document (`spec` field).
pub const EXPORT_SPEC: &str = "fotobase.export";
/// Export document shape version.
pub const EXPORT_VERSION: u32 = 1;
/// Discriminator of an edit recipe (docs/edit-recipe-spec-v1.md §1).
pub const RECIPE_SPEC: &str = "fotobase.recipe";

/// Op types of recipe spec v1 (docs/recipe-v1.schema.json `opType`).
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Json,
    Csv,
}

/// Which user data to export; at least one must be picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportSections {
    pub albums: bool,
    pub tags: bool,
    pub recipes: bool,
}

/// The export document. A section is `None` when it was not picked, and
/// `Some(vec![])` when it was picked but empty.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PortableExport {
    pub spec: String,
    pub version: u32,
    /// UTC, `YYYY-MM-DDTHH:MM:SSZ`.
    pub exported_at: String,
    pub generator: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub albums: Option<Vec<ExportedAlbum>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tags: Option<Vec<ExportedTag>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recipes: Option<Vec<ExportedRecipe>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub unreadable_recipes: Vec<UnreadableRecipe>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedAlbum {
    /// Stable UUID written to members' `fb:Albums`.
    pub id: String,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub place: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// Member photo paths in album order.
    pub members: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedTag {
    pub name: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExportedRecipe {
    pub file: String,
    /// A conforming recipe packet (docs/recipe-v1.schema.json).
    pub recipe: Value,
}

/// An `fb:Recipe` that is not a conforming v1 packet: kept verbatim rather
/// than dropped, with the reason it could not be exported as a recipe.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnreadableRecipe {
    pub file: String,
    pub raw: String,
    pub reason: String,
}

/// What an export wrote.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExportReport {
    pub files: Vec<PathBuf>,
    pub albums: usize,
    pub tags: usize,
    pub recipes: usize,
    /// Recipes kept raw in the JSON (not written to CSV).
    pub unreadable_recipes: usize,
}

impl PortableExport {
    /// Parse an export document, refusing anything that is not a Fotobase
    /// export of a version this build understands.
    pub fn from_json(text: &str) -> serde_json::Result<PortableExport> {
        use serde::de::Error as _;
        let export: PortableExport = serde_json::from_str(text)?;
        if export.spec != EXPORT_SPEC {
            return Err(serde_json::Error::custom(format!("not a {EXPORT_SPEC} document")));
        }
        if export.version == 0 || export.version > EXPORT_VERSION {
            return Err(serde_json::Error::custom(format!(
                "unsupported export version {}",
                export.version
            )));
        }
        Ok(export)
    }
}

impl Catalog {
    /// Build the export document from the catalog and the recipe sidecars.
    /// Hidden photos are included: hiding is a view, not a deletion.
    pub fn portable_export(&self, sections: ExportSections) -> Result<PortableExport> {
        let mut export = PortableExport {
            spec: EXPORT_SPEC.to_string(),
            version: EXPORT_VERSION,
            exported_at: utc_timestamp(SystemTime::now()),
            generator: format!("Fotobase {}", env!("CARGO_PKG_VERSION")),
            albums: None,
            tags: None,
            recipes: None,
            unreadable_recipes: Vec::new(),
        };
        if sections.albums {
            export.albums = Some(self.export_albums()?);
        }
        if sections.tags {
            export.tags = Some(self.export_tags()?);
        }
        if sections.recipes {
            let (recipes, unreadable) = self.export_recipes()?;
            export.recipes = Some(recipes);
            export.unreadable_recipes = unreadable;
        }
        Ok(export)
    }

    /// Export the picked sections into `dir` (an existing folder) as one
    /// JSON document or one CSV table per section. File names carry the
    /// export time and never replace an existing file.
    pub fn export_user_data(
        &self,
        dir: &Path,
        sections: ExportSections,
        format: ExportFormat,
    ) -> Result<ExportReport> {
        if !(sections.albums || sections.tags || sections.recipes) {
            return Err(CatalogError::Export("nothing selected to export".into()));
        }
        if !dir.is_dir() {
            return Err(CatalogError::Export(format!("not a folder: {}", dir.display())));
        }
        let export = self.portable_export(sections)?;

        let mut outputs: Vec<(&str, Vec<u8>)> = Vec::new();
        match format {
            ExportFormat::Json => {
                let mut json = serde_json::to_vec_pretty(&export)
                    .map_err(|e| CatalogError::Export(e.to_string()))?;
                json.push(b'\n');
                outputs.push((".json", json));
            }
            ExportFormat::Csv => {
                if let Some(albums) = &export.albums {
                    outputs.push(("-albums.csv", albums_csv(albums).into_bytes()));
                }
                if let Some(tags) = &export.tags {
                    outputs.push(("-tags.csv", tags_csv(tags).into_bytes()));
                }
                if let Some(recipes) = &export.recipes {
                    outputs.push(("-recipes.csv", recipes_csv(recipes).into_bytes()));
                }
            }
        }

        let stem = unique_stem(dir, &export.exported_at, outputs.iter().map(|(s, _)| *s));
        let mut files = Vec::with_capacity(outputs.len());
        for (suffix, bytes) in outputs {
            let path = dir.join(format!("{stem}{suffix}"));
            write_new_file(&path, &bytes)?;
            files.push(path);
        }
        Ok(ExportReport {
            files,
            albums: export.albums.as_ref().map_or(0, Vec::len),
            tags: export.tags.as_ref().map_or(0, Vec::len),
            recipes: export.recipes.as_ref().map_or(0, Vec::len),
            unreadable_recipes: export.unreadable_recipes.len(),
        })
    }

    fn export_albums(&self) -> Result<Vec<ExportedAlbum>> {
        let mut stmt = self.conn().prepare(
            "SELECT id, uuid, name, date, place, description FROM albums
             ORDER BY date DESC, name COLLATE NOCASE, id",
        )?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                ExportedAlbum {
                    id: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
                    name: r.get(2)?,
                    date: r.get(3)?,
                    place: r.get(4)?,
                    description: r.get(5)?,
                    members: Vec::new(),
                },
            ))
        })?;
        let mut members = self.conn().prepare(
            "SELECT f.path FROM album_files af JOIN files f ON f.id = af.file_id
             WHERE af.album_id = ?1 ORDER BY af.position, af.file_id",
        )?;
        let mut albums = Vec::new();
        for row in rows {
            let (id, mut album) = row?;
            album.members =
                members.query_map([id], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
            albums.push(album);
        }
        Ok(albums)
    }

    fn export_tags(&self) -> Result<Vec<ExportedTag>> {
        let mut stmt = self.conn().prepare(
            "SELECT t.name, f.path FROM tags t
             JOIN file_tags ft ON ft.tag_id = t.id JOIN files f ON f.id = ft.file_id
             ORDER BY t.name COLLATE NOCASE, t.id, f.path",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))?;
        let mut tags: Vec<ExportedTag> = Vec::new();
        for row in rows {
            let (name, path) = row?;
            match tags.last_mut() {
                Some(tag) if tag.name == name => tag.files.push(path),
                _ => tags.push(ExportedTag { name, files: vec![path] }),
            }
        }
        Ok(tags)
    }

    fn export_recipes(&self) -> Result<(Vec<ExportedRecipe>, Vec<UnreadableRecipe>)> {
        let mut stmt =
            self.conn().prepare("SELECT path FROM files WHERE has_recipe = 1 ORDER BY path")?;
        let paths: Vec<String> =
            stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?;
        let (mut recipes, mut unreadable) = (Vec::new(), Vec::new());
        for file in paths {
            let raw = match Sidecar::read(&sidecar_path(&file)) {
                Ok(sidecar) => match sidecar.recipe() {
                    Some(raw) => raw,
                    // Removed since the last sync: nothing to export.
                    None => continue,
                },
                Err(e) => {
                    unreadable.push(UnreadableRecipe {
                        file,
                        raw: String::new(),
                        reason: format!("sidecar unreadable: {e}"),
                    });
                    continue;
                }
            };
            match conform_recipe(&raw) {
                Ok(recipe) => recipes.push(ExportedRecipe { file, recipe }),
                Err(reason) => unreadable.push(UnreadableRecipe { file, raw, reason }),
            }
        }
        Ok((recipes, unreadable))
    }
}

/// Parse an `fb:Recipe` packet into a schema-conforming v1 recipe. A packet
/// missing only the `spec` discriminator (written before it was pinned)
/// gains it; anything else non-conforming is rejected with a reason.
fn conform_recipe(raw: &str) -> std::result::Result<Value, String> {
    let mut value: Value = serde_json::from_str(raw).map_err(|e| format!("not JSON: {e}"))?;
    let obj = value.as_object_mut().ok_or("not a JSON object")?;
    if let Some(key) = obj.keys().find(|k| !matches!(k.as_str(), "spec" | "version" | "ops")) {
        return Err(format!("unknown field `{key}`"));
    }
    match obj.get("spec") {
        None => {
            obj.insert("spec".into(), Value::String(RECIPE_SPEC.into()));
        }
        Some(Value::String(s)) if s == RECIPE_SPEC => {}
        Some(other) => return Err(format!("spec is {other}, not \"{RECIPE_SPEC}\"")),
    }
    if !obj.get("version").and_then(Value::as_u64).is_some_and(|v| v >= 1) {
        return Err("missing or invalid version".into());
    }
    let ops = obj.get("ops").and_then(Value::as_array).ok_or("missing ops array")?;
    for op in ops {
        let op: &Map<String, Value> = op.as_object().ok_or("op is not an object")?;
        let ty = op.get("type").and_then(Value::as_str).ok_or("op without a type")?;
        if !V1_OP_TYPES.contains(&ty) {
            return Err(format!("op type `{ty}` is not in recipe spec v1"));
        }
        if op.get("params").is_some_and(|p| !p.is_object()) {
            return Err(format!("params of `{ty}` is not an object"));
        }
    }
    Ok(value)
}

const ALBUMS_HEADER: &str =
    "album_id,album_name,album_date,place,description,position,filename,path";

fn albums_csv(albums: &[ExportedAlbum]) -> String {
    let mut out = csv_row(ALBUMS_HEADER.split(','));
    for a in albums {
        let head = [
            a.id.as_str(),
            &a.name,
            a.date.as_deref().unwrap_or(""),
            a.place.as_deref().unwrap_or(""),
            a.description.as_deref().unwrap_or(""),
        ];
        if a.members.is_empty() {
            out.push_str(&csv_row(head.into_iter().chain(["", "", ""])));
        }
        for (i, path) in a.members.iter().enumerate() {
            let position = (i + 1).to_string();
            out.push_str(&csv_row(head.into_iter().chain([
                position.as_str(),
                file_name(path),
                path.as_str(),
            ])));
        }
    }
    out
}

fn tags_csv(tags: &[ExportedTag]) -> String {
    let mut out = csv_row(["tag", "filename", "path"]);
    for tag in tags {
        for path in &tag.files {
            out.push_str(&csv_row([tag.name.as_str(), file_name(path), path.as_str()]));
        }
    }
    out
}

fn recipes_csv(recipes: &[ExportedRecipe]) -> String {
    let mut out = csv_row(["filename", "path", "recipe"]);
    for r in recipes {
        let json = r.recipe.to_string();
        out.push_str(&csv_row([file_name(&r.file), r.file.as_str(), json.as_str()]));
    }
    out
}

fn file_name(path: &str) -> &str {
    Path::new(path).file_name().and_then(|n| n.to_str()).unwrap_or(path)
}

/// One RFC 4180 record (CRLF-terminated; fields with `,` `"` CR or LF quoted).
fn csv_row<'a>(fields: impl IntoIterator<Item = &'a str>) -> String {
    let mut row = String::new();
    for (i, field) in fields.into_iter().enumerate() {
        if i > 0 {
            row.push(',');
        }
        if field.contains([',', '"', '\r', '\n']) {
            row.push('"');
            row.push_str(&field.replace('"', "\"\""));
            row.push('"');
        } else {
            row.push_str(field);
        }
    }
    row.push_str("\r\n");
    row
}

/// `fotobase-export-YYYYMMDD-HHMMSS`, suffixed `-2`, `-3`… until none of
/// the export's files would collide with an existing one.
fn unique_stem<'a>(
    dir: &Path,
    exported_at: &str,
    suffixes: impl Iterator<Item = &'a str>,
) -> String {
    let suffixes: Vec<&str> = suffixes.collect();
    let compact: String = exported_at.chars().filter(char::is_ascii_digit).collect();
    let (date, time) = compact.split_at(compact.len().min(8));
    let base = format!("fotobase-export-{date}-{time}");
    let mut n = 1;
    loop {
        let stem = if n == 1 { base.clone() } else { format!("{base}-{n}") };
        let taken = suffixes.iter().any(|s| {
            let path = dir.join(format!("{stem}{s}"));
            path.exists() || tmp_path(&path).exists()
        });
        if !taken {
            return stem;
        }
        n += 1;
    }
}

fn tmp_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".tmp");
    path.with_file_name(name)
}

/// Write via `.tmp` + rename so a crash never leaves a truncated export.
fn write_new_file(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = tmp_path(path);
    let written = (|| {
        let mut f = std::fs::File::create_new(&tmp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
        std::fs::rename(&tmp, path)
    })();
    if let Err(e) = written {
        let _ = std::fs::remove_file(&tmp);
        return Err(CatalogError::Export(format!("writing {}: {e}", path.display())));
    }
    Ok(())
}

fn utc_timestamp(t: SystemTime) -> String {
    let secs = t.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_secs() as i64;
    let (y, m, d) = civil_from_days(secs.div_euclid(86_400));
    let tod = secs.rem_euclid(86_400);
    format!("{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z", tod / 3600, (tod % 3600) / 60, tod % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn v1_op_types_match_the_published_schema() {
        let schema: Value =
            serde_json::from_str(include_str!("../../../docs/recipe-v1.schema.json")).unwrap();
        let published: Vec<&str> = schema["$defs"]["opType"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap())
            .collect();
        assert_eq!(published, V1_OP_TYPES);
    }

    #[test]
    fn utc_timestamp_is_rfc3339_zulu() {
        let t = SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_721_000_000);
        assert_eq!(utc_timestamp(t), "2024-07-14T23:33:20Z");
    }

    #[test]
    fn csv_quotes_only_when_needed() {
        assert_eq!(
            csv_row(["a", "b c", "d,e", "say \"hi\"", "x\ny"]),
            "a,b c,\"d,e\",\"say \"\"hi\"\"\",\"x\ny\"\r\n"
        );
    }

    #[test]
    fn conform_recipe_rejects_foreign_shapes() {
        for bad in [
            "[]",
            r#"{"spec":"other","version":1,"ops":[]}"#,
            r#"{"version":0,"ops":[]}"#,
            r#"{"version":1}"#,
            r#"{"version":1,"ops":[{"params":{}}]}"#,
            r#"{"version":1,"ops":[{"type":"sepia","params":3}]}"#,
            r#"{"version":1,"ops":[],"extra":true}"#,
        ] {
            assert!(conform_recipe(bad).is_err(), "{bad}");
        }
        assert!(conform_recipe(r#"{"version":1,"ops":[]}"#).is_ok());
    }
}
