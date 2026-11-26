//! TASK-10-02-02: lens correction through the native Lensfun library and
//! database (PLAN §7 F2 item 2). The profile is resolved once from EXIF and
//! stored in the recipe as Lensfun XML, so a render never depends on which
//! database version is installed.
#![cfg(all(feature = "lensfun", fb_native_codecs))]

use fotobase_core::buffer::LinearImage;
use fotobase_core::recipe::Recipe;
use fotobase_render::lensfun::{LensDb, LensQuery};
use serde_json::{json, Value};

const TEST_DB: &str = r#"<lensdatabase version="1">
  <camera><maker>Testcam</maker><model>Testcam One</model><mount>Testmount</mount><cropfactor>1</cropfactor></camera>
  <camera><maker>Testcam</maker><model>Testcam Crop</model><mount>Testmount</mount><cropfactor>1.5</cropfactor></camera>
  <lens>
    <maker>Testlens</maker><model>Testlens 50mm f/2</model><mount>Testmount</mount><cropfactor>1</cropfactor>
    <calibration>
      <distortion model="poly3" focal="50" k1="-0.05"/>
      <vignetting model="pa" focal="50" aperture="4" distance="1000" k1="-0.3" k2="0" k3="0"/>
    </calibration>
  </lens>
  <lens>
    <maker>Testlens</maker><model>Testlens 50mm f/2</model><mount>Testmount</mount><cropfactor>1.5</cropfactor>
    <calibration><distortion model="poly3" focal="50" k1="0.02"/></calibration>
  </lens>
  <lens>
    <maker>Testlens</maker><model>Testlens 24mm f/2.8</model><mount>Testmount</mount><cropfactor>1</cropfactor>
    <calibration><distortion model="poly3" focal="24" k1="-0.1"/></calibration>
  </lens>
</lensdatabase>"#;

fn query(camera: &str, lens: &str) -> LensQuery {
    LensQuery {
        camera_maker: "Testcam".into(),
        camera_model: camera.into(),
        lens_maker: None,
        lens_model: Some(lens.into()),
        focal: Some(50.0),
        aperture: Some(4.0),
        distance: None,
    }
}

fn render(img: &LinearImage, params: &Value) -> LinearImage {
    let packet = json!({ "spec": "fotobase.recipe", "version": 1,
                         "ops": [{ "type": "lens_correction", "params": params }] });
    let (recipe, _) = Recipe::parse(&packet.to_string()).expect("lens op parses");
    fotobase_render::apply_recipe(img, &recipe)
}

#[test]
fn exif_camera_and_lens_select_the_profile_for_that_crop_factor() {
    let db = LensDb::from_xml(TEST_DB).expect("test DB loads");
    let params = db.profile(&query("Testcam Crop", "Testlens 50mm f/2")).expect("lens found");
    assert_eq!(params["lens"], "Testlens 50mm f/2");
    assert_eq!(params["crop"], 1.5);
    assert_eq!(params["focal"], 50.0);
    assert_eq!(params["aperture"], 4.0);
    let profile = params["profile"].as_str().unwrap();
    assert!(profile.contains("0.02"), "APS-C calibration expected: {profile}");
    assert!(!profile.contains("-0.05"), "full-frame calibration must not leak in");

    let (recipe, warnings) = Recipe::parse(
        &json!({ "spec": "fotobase.recipe", "version": 1,
                 "ops": [{ "type": "lens_correction", "params": params }] })
        .to_string(),
    )
    .expect("resolved profile is a valid recipe op");
    assert_eq!(recipe.ops.len(), 1);
    assert!(warnings.is_empty(), "{warnings:?}");
}

#[test]
fn missing_lens_data_is_an_explicit_error_never_a_guess() {
    let db = LensDb::from_xml(TEST_DB).unwrap();
    let unknown_lens = db.profile(&query("Testcam One", "Nolens 85mm")).unwrap_err();
    assert!(unknown_lens.to_string().contains("Nolens 85mm"), "{unknown_lens}");
    let unknown_camera = db.profile(&query("Nocam 9", "Testlens 50mm f/2")).unwrap_err();
    assert!(unknown_camera.to_string().contains("Nocam 9"), "{unknown_camera}");
    let no_lens = LensQuery { lens_model: None, ..query("Testcam One", "") };
    assert!(db.profile(&no_lens).is_err());
    let no_focal = LensQuery { focal: None, ..query("Testcam One", "Testlens 50mm f/2") };
    assert!(db.profile(&no_focal).is_err());
}

/// Independent check against Lensfun's documented `pa` vignetting model:
/// the source was darkened by 1 + k1·r², r = 1 at the half diagonal, so the
/// correction must divide a flat field's corners by ≈ 1 − 0.3.
#[test]
fn vignetting_correction_follows_the_pa_model() {
    let db = LensDb::from_xml(TEST_DB).unwrap();
    let mut params = db.profile(&query("Testcam One", "Testlens 50mm f/2")).unwrap();
    params["distortion"] = json!(false);
    params["tca"] = json!(false);
    let flat = LinearImage::from_fn(64, 64, |_, _| [0.5; 3]);
    let out = render(&flat, &params);
    let center = out.get(32, 32)[1];
    let corner = out.get(0, 0)[1];
    assert!((center - 0.5).abs() < 0.01, "center {center}");
    let r2 = (31.5f32.powi(2) * 2.0) / (32.0f32.powi(2) * 2.0);
    let expected = 0.5 / (1.0 - 0.3 * r2);
    assert!((corner - expected).abs() < 0.02, "corner {corner}, expected {expected}");
}

#[test]
fn distortion_correction_moves_pixels_and_matches_across_resolutions() {
    let db = LensDb::from_xml(TEST_DB).unwrap();
    let mut params = db.profile(&query("Testcam One", "Testlens 50mm f/2")).unwrap();
    params["vignetting"] = json!(false);
    let scene = |w: u32, h: u32| {
        LinearImage::from_fn(w, h, |x, y| {
            let (fx, fy) = ((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
            [fx, fy, 0.5 + 0.4 * (fx * 12.0).sin() * (fy * 9.0).cos()]
        })
    };
    let (small, big) = (scene(96, 64), scene(384, 256));
    let out_small = render(&small, &params);
    let out_big = render(&big, &params);
    assert_ne!(out_small.data, small.data, "a poly3 profile must move pixels");
    let mut worst = 0.0f32;
    for y in 4..60 {
        for x in 4..92 {
            let mut avg = [0.0f32; 3];
            for dy in 0..4 {
                for dx in 0..4 {
                    let p = out_big.get(x * 4 + dx, y * 4 + dy);
                    (0..3).for_each(|c| avg[c] += p[c] / 16.0);
                }
            }
            let p = out_small.get(x, y);
            (0..3).for_each(|c| worst = worst.max((avg[c] - p[c]).abs()));
        }
    }
    assert!(worst < 0.03, "preview/export diverge by {worst}");
}

#[test]
fn the_installed_lensfun_database_resolves_a_common_lens() {
    let db = LensDb::system().expect("Lensfun DB installed with the library");
    let params = db
        .profile(&LensQuery {
            camera_maker: "Canon".into(),
            camera_model: "Canon EOS 5D Mark II".into(),
            lens_maker: Some("Canon".into()),
            lens_model: Some("Canon EF 50mm f/1.8 II".into()),
            focal: Some(50.0),
            aperture: Some(2.8),
            distance: None,
        })
        .expect("Canon EF 50mm f/1.8 II is in the Lensfun DB");
    assert_eq!(params["crop"], 1.0);
    assert!(params["lens"].as_str().unwrap().contains("50mm"));
}

#[test]
fn corrected_pixels_stay_in_the_linear_light_range() {
    let db = LensDb::from_xml(TEST_DB).unwrap();
    let params = db.profile(&query("Testcam One", "Testlens 50mm f/2")).unwrap();
    let bright = LinearImage::from_fn(32, 32, |_, _| [0.95; 3]);
    let out = render(&bright, &params);
    assert!(out.data.iter().all(|v| (0.0..=1.0).contains(v)), "vignetting overshoot leaked");
}
