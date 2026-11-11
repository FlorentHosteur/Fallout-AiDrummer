//! Moteur d'AI Drummer : analyse d'un morceau et, plus tard, écriture de la batterie.
//!
//! Ce crate ne dépend pas de Reaper : il se teste et s'utilise seul (voir le binaire `aidrummer`).

pub mod accents;
pub mod advice;
pub mod ai;
pub mod analysis;
pub mod audio;
pub mod drums;
pub mod onsets;
pub mod report;

pub use accents::{accent_map, template_accents, AccentMap, BarAccents, Grid, ProjectSpan};
pub use advice::{advice_report, advise, apply_advice, Advice, Edit};
pub use ai::{ask_advice, ask_plan, list_models, AiSettings};
pub use analysis::{analyze, grid_fit, GridFit, SongAnalysis};
pub use audio::{decode_mono, Audio};
pub use drums::{
    piece_of, read_drums, sketch_drums, write_drums, DrumLane, DrumNote, DrumPlan, DrumReading,
    Feel, Hat, Piece,
};
pub use onsets::{detect_onsets, detect_onsets_after_preroll, Onset};
pub use report::{selection_report, SelectionAnalysis, TrackAccents};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("lecture du fichier audio impossible : {0}")]
    Io(#[from] std::io::Error),
    #[error("décodage audio impossible : {0}")]
    Decode(String),
    #[error("analyse impossible : {0}")]
    Analysis(String),
}
