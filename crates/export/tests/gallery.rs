//! TDD tests for TASK-08-016: the static HTML gallery generator produces a
//! self-contained, offline site and re-exports incrementally (PLAN §2 §J
//! "HTML export", §7 F1 layer 1, §9 F4 check).

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use fotobase_export::gallery::{
    builtin_template, builtin_templates, generate, GalleryError, GalleryOptions, GalleryPhoto,
    RenderPipeline,
};

fn write_jpeg(path: &Path, w: u32, h: u32, seed: u8) {
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x as u8).wrapping_add(seed), (y as u8).wrapping_mul(2), seed])
    });
    img.save_with_format(path, image::ImageFormat::Jpeg).unwrap();
}

/// Three fixture photos; the second has no caption or title.
fn fixture_album(dir: &Path) -> Vec<GalleryPhoto> {
    let mut photos = Vec::new();
    for (i, (name, title, caption)) in [
        ("a.jpg", Some("Sunset"), Some("Evening at the <beach> & dunes")),
        ("b.jpg", None, None),
        ("c.jpg", Some("Hike"), Some("Summit")),
    ]
    .into_iter()
    .enumerate()
    {
        let path = dir.join(name);
        write_jpeg(&path, 400 + 40 * i as u32, 300, 10 + 50 * i as u8);
        photos.push(GalleryPhoto {
            source: path,
            title: title.map(str::to_owned),
            caption: caption.map(str::to_owned),
            recipe: None,
        });
    }
    photos
}

fn options(template: &str) -> GalleryOptions {
    GalleryOptions {
        title: "Summer 2026".into(),
        description: Some("Holiday & more".into()),
        template: template.into(),
        image_max_edge: 256,
        thumb_max_edge: 64,
        ..GalleryOptions::default()
    }
}

fn snapshot(root: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<PathBuf, Vec<u8>>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_path_buf(),
                    fs::read(&path).unwrap(),
                );
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// Every `src`/`href` attribute and CSS `url(...)` in the site's text files.
fn references(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    for marker in ["src=\"", "href=\"", "url("] {
        let mut rest = text;
        while let Some(at) = rest.find(marker) {
            rest = &rest[at + marker.len()..];
            let end = rest.find(['"', ')']).unwrap_or(rest.len());
            found.push(rest[..end].trim_matches('\'').to_owned());
            rest = &rest[end..];
        }
    }
    found
}

#[test]
fn export_writes_a_self_contained_site() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let photos = fixture_album(src.path());
    let template = builtin_template("grid").unwrap();

    let report = generate(out.path(), &photos, &options("grid"), &template, &RenderPipeline)
        .expect("gallery export");
    assert_eq!(report.photos, 3);

    let files = snapshot(out.path());
    let names: Vec<String> = files.keys().map(|p| p.to_string_lossy().replace('\\', "/")).collect();
    assert!(names.contains(&"index.html".to_owned()), "{names:?}");
    assert!(names.contains(&"viewer.js".to_owned()), "{names:?}");
    let images: Vec<_> = names.iter().filter(|n| n.starts_with("images/")).collect();
    let thumbs: Vec<_> = names.iter().filter(|n| n.starts_with("thumbs/")).collect();
    assert_eq!((images.len(), thumbs.len()), (3, 3), "{names:?}");

    // Resized: bounded by the requested edges, never upscaled.
    for name in &images {
        let (w, h) = image::image_dimensions(out.path().join(name)).unwrap();
        assert!(w.max(h) <= 256 && w.max(h) > 64, "{name}: {w}x{h}");
    }
    for name in &thumbs {
        let (w, h) = image::image_dimensions(out.path().join(name)).unwrap();
        assert!(w.max(h) <= 64, "{name}: {w}x{h}");
    }

    // The sources are never touched or copied verbatim.
    for photo in &photos {
        assert!(photo.source.exists());
    }
}

#[test]
fn site_has_zero_network_references_and_every_reference_resolves() {
    let src = tempfile::tempdir().unwrap();
    let photos = fixture_album(src.path());

    for template in builtin_templates() {
        let out = tempfile::tempdir().unwrap();
        generate(out.path(), &photos, &options(&template.id), &template, &RenderPipeline).unwrap();

        for (path, bytes) in snapshot(out.path()) {
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if !matches!(ext, "html" | "css" | "js") {
                continue;
            }
            let text = String::from_utf8(bytes).unwrap();
            for needle in ["http://", "https://", "ws://", "wss://", "fetch(", "XMLHttpRequest"] {
                assert!(!text.contains(needle), "{}: contains {needle}", path.display());
            }
            if ext == "js" {
                continue;
            }
            for reference in references(&text) {
                assert!(
                    !reference.contains("://") && !reference.starts_with("//"),
                    "{}: remote reference {reference}",
                    path.display()
                );
                if reference.starts_with('#') || reference.is_empty() {
                    continue;
                }
                let base = path.parent().unwrap();
                assert!(
                    out.path().join(base).join(&reference).is_file(),
                    "{}: dangling reference {reference}",
                    path.display()
                );
            }
        }
    }
}

#[test]
fn captions_and_titles_render_escaped() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let photos = fixture_album(src.path());
    let template = builtin_template("grid").unwrap();
    generate(out.path(), &photos, &options("grid"), &template, &RenderPipeline).unwrap();

    let html = fs::read_to_string(out.path().join("index.html")).unwrap();
    assert!(html.contains("<title>Summer 2026</title>"), "{html}");
    assert!(html.contains("Holiday &amp; more"), "{html}");
    assert!(html.contains("Sunset"));
    assert!(html.contains("Evening at the &lt;beach&gt; &amp; dunes"), "{html}");
    assert!(!html.contains("<beach>"), "caption must be escaped");
    assert!(html.contains("Summit"));
    // The embedded viewer data carries the same captions, JSON-escaped.
    assert!(html.contains("application/json"));
    assert!(html.contains("Evening at the \\u003cbeach\\u003e"), "{html}");
}

#[test]
fn ships_at_least_two_selectable_templates_with_different_output() {
    let templates = builtin_templates();
    assert!(templates.len() >= 2);
    let ids: Vec<_> = templates.iter().map(|t| t.id.as_str()).collect();
    assert!(ids.contains(&"grid") && ids.contains(&"dark"), "{ids:?}");

    let src = tempfile::tempdir().unwrap();
    let photos = fixture_album(src.path());
    let mut pages = Vec::new();
    for template in &templates {
        let out = tempfile::tempdir().unwrap();
        generate(out.path(), &photos, &options(&template.id), template, &RenderPipeline).unwrap();
        pages.push(fs::read_to_string(out.path().join("index.html")).unwrap());
    }
    assert_ne!(pages[0], pages[1]);
}

#[test]
fn re_export_is_byte_identical_and_rewrites_nothing() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let photos = fixture_album(src.path());
    let template = builtin_template("grid").unwrap();

    let first =
        generate(out.path(), &photos, &options("grid"), &template, &RenderPipeline).unwrap();
    assert!(first.written > 0 && first.unchanged == 0);
    let before = snapshot(out.path());

    let second =
        generate(out.path(), &photos, &options("grid"), &template, &RenderPipeline).unwrap();
    assert_eq!(second.written, 0, "{second:?}");
    assert_eq!(second.removed, 0);
    assert_eq!(second.unchanged, first.written);
    assert_eq!(snapshot(out.path()), before);
}

#[test]
fn re_export_after_one_photo_changes_only_that_photos_files_differ() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let photos = fixture_album(src.path());
    let template = builtin_template("grid").unwrap();
    generate(out.path(), &photos, &options("grid"), &template, &RenderPipeline).unwrap();
    let before = snapshot(out.path());

    write_jpeg(&photos[1].source, 440, 300, 200);
    let report =
        generate(out.path(), &photos, &options("grid"), &template, &RenderPipeline).unwrap();
    let after = snapshot(out.path());

    let changed: Vec<_> = after
        .iter()
        .filter(|(path, bytes)| before.get(*path) != Some(*bytes))
        .map(|(p, _)| p.to_string_lossy().replace('\\', "/"))
        .collect();
    let removed: Vec<_> = before
        .keys()
        .filter(|p| !after.contains_key(*p))
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .collect();

    // New image + thumbnail for the edited photo, plus the page and manifest
    // that link them; the old pair is pruned. The other two photos keep
    // their files.
    assert_eq!(changed.len(), 4, "{changed:?}");
    assert!(changed.contains(&"index.html".to_owned()));
    assert!(changed.contains(&"manifest.json".to_owned()));
    assert_eq!(changed.iter().filter(|c| c.starts_with("images/")).count(), 1);
    assert_eq!(changed.iter().filter(|c| c.starts_with("thumbs/")).count(), 1);
    assert_eq!(removed.len(), 2, "{removed:?}");
    assert_eq!(report.removed, 2);
    assert_eq!(after.len(), before.len());
}

#[test]
fn prune_leaves_unrelated_files_in_the_destination_alone() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let photos = fixture_album(src.path());
    let template = builtin_template("grid").unwrap();
    fs::create_dir_all(out.path().join("images")).unwrap();
    fs::write(out.path().join("images/notes.txt"), "mine").unwrap();
    fs::write(out.path().join("robots.txt"), "mine").unwrap();

    generate(out.path(), &photos, &options("grid"), &template, &RenderPipeline).unwrap();
    assert_eq!(fs::read_to_string(out.path().join("images/notes.txt")).unwrap(), "mine");
    assert_eq!(fs::read_to_string(out.path().join("robots.txt")).unwrap(), "mine");
}

#[test]
fn rejects_empty_selection_unknown_template_and_unreadable_source() {
    let src = tempfile::tempdir().unwrap();
    let out = tempfile::tempdir().unwrap();
    let template = builtin_template("grid").unwrap();

    let err = generate(out.path(), &[], &options("grid"), &template, &RenderPipeline).unwrap_err();
    assert!(matches!(err, GalleryError::NoPhotos), "{err}");

    assert!(builtin_template("nope").is_none());

    let missing = GalleryPhoto {
        source: src.path().join("missing.jpg"),
        title: None,
        caption: None,
        recipe: None,
    };
    let err =
        generate(out.path(), &[missing], &options("grid"), &template, &RenderPipeline).unwrap_err();
    assert!(matches!(err, GalleryError::Io(_)), "{err}");
}
