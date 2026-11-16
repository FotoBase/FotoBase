//! CLIP keyword extraction for auto-tagging (TASK-07-004, PLAN §6 P3
//! "Auto-tagging: content keywords generated from embeddings, written as
//! normal tags").
//!
//! The image embedding (stored by the 07-001 pass in the catalog's
//! `photo_embeddings` table) is compared against *prompt embeddings*: each
//! vocabulary concept is embedded as `"a photo of {concept}"` with the same
//! CLIP text tower the semantic search uses (TASK-07-002 — no new model).
//! Concepts within the cosine-distance threshold become suggestions.
//!
//! The vocabulary is a data file (`vocabulary.json`), embeddable at compile
//! time but extendable without code changes.

use serde::Deserialize;

/// The embedded vocabulary data file (TASK-07-004 technical spec: extendable
/// without code changes).
pub const VOCABULARY_JSON: &str = include_str!("../vocabulary.json");

#[derive(Debug, Deserialize, PartialEq)]
pub struct Vocabulary {
    /// Cosine-distance threshold: a concept closer than this to the image
    /// embedding becomes a suggestion.
    pub threshold: f32,
    /// The fixed concept list.
    pub concepts: Vec<String>,
}

impl Vocabulary {
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// The prompt for one concept, in OpenCLIP's prompt style.
    pub fn prompt(&self, concept: &str) -> String {
        format!("a photo of {concept}")
    }

    pub fn prompts(&self) -> Vec<(String, String)> {
        self.concepts.iter().map(|c| (c.clone(), self.prompt(c))).collect()
    }
}

/// One extracted keyword suggestion, closest first.
#[derive(Debug, Clone, PartialEq)]
pub struct KeywordHit {
    pub concept: String,
    /// Cosine distance (1 − dot product; both vectors are L2-normalized).
    pub distance: f32,
}

/// Nearest-neighbor keyword extraction (TASK-07-004 technical spec): rank
/// the vocabulary prompt embeddings by cosine distance against the image
/// embedding and keep those under the threshold, closest first. Pure
/// function over normalized vectors — testable without the model.
pub fn extract_keywords(
    image: &[f32],
    prompts: &[(String, Vec<f32>)],
    threshold: f32,
) -> Vec<KeywordHit> {
    let mut hits: Vec<KeywordHit> = prompts
        .iter()
        .filter(|(_, v)| v.len() == image.len())
        .map(|(concept, v)| {
            let dot: f32 = image.iter().zip(v).map(|(a, b)| a * b).sum();
            KeywordHit { concept: concept.clone(), distance: 1.0 - dot }
        })
        .filter(|h| h.distance < threshold)
        .collect();
    hits.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unit(seed: f32) -> Vec<f32> {
        use crate::CLIP_EMBEDDING_DIM;
        let mut v: Vec<f32> = (0..CLIP_EMBEDDING_DIM).map(|i| (i as f32 + seed).sin()).collect();
        let norm: f32 = v.iter().map(|x| x * x).sum::<f32>().sqrt();
        for x in &mut v {
            *x /= norm;
        }
        v
    }

    fn vocab() -> Vocabulary {
        Vocabulary::from_json(VOCABULARY_JSON).unwrap()
    }

    #[test]
    fn vocabulary_loads_from_the_data_file() {
        let v = vocab();
        assert!(v.threshold > 0.0 && v.threshold < 1.0);
        assert!(v.concepts.contains(&"dog".to_string()), "the TDD integration fixture concept");
        assert!(!v.concepts.is_empty());
        // Every prompt is the fixed template; concepts stay lowercase.
        assert_eq!(v.prompt("dog"), "a photo of dog");
        assert!(v.concepts.iter().all(|c| c == &c.to_lowercase()));
    }

    #[test]
    fn nearest_concept_wins_and_threshold_filters() {
        let image = unit(0.0);
        let prompts = vec![
            ("dog".to_string(), unit(0.1)), // very close
            ("cat".to_string(), unit(0.4)), // mid
            ("far".to_string(), unit(3.0)), // beyond any threshold
        ];
        let hits = extract_keywords(&image, &prompts, 0.5);
        assert_eq!(hits.first().unwrap().concept, "dog", "closest concept leads");
        assert!(!hits.iter().any(|h| h.concept == "far"), "beyond-threshold concepts are dropped");
        // Distances ascend with the list.
        let d: Vec<f32> = hits.iter().map(|h| h.distance).collect();
        assert!(d.windows(2).all(|w| w[0] <= w[1]), "closest first: {d:?}");
        // A zero threshold keeps nothing.
        assert!(extract_keywords(&image, &prompts, 0.0).is_empty());
    }

    #[test]
    fn embedded_vocabulary_and_prompts_agree() {
        let v = vocab();
        let prompts = v.prompts();
        assert_eq!(prompts.len(), v.concepts.len());
        assert!(prompts.iter().all(|(c, p)| p == &format!("a photo of {c}")));
    }

    #[test]
    fn malformed_vocabulary_is_rejected() {
        assert!(Vocabulary::from_json("{").is_err());
    }
}
