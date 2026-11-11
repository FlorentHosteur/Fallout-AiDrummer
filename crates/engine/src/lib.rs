//! Moteur d'AI Drummer : analyse d'un morceau et, plus tard, écriture de la batterie.
//!
//! Ce crate ne dépend pas de Reaper : il se teste et s'utilise seul (voir le binaire `aidrummer`).

pub mod analysis;
pub mod audio;

pub use analysis::{analyze, grid_fit, GridFit, SongAnalysis};
pub use audio::{decode_mono, Audio};

#[derive(Debug, thiserror::Error)]
pub enum EngineError {
    #[error("lecture du fichier audio impossible : {0}")]
    Io(#[from] std::io::Error),
    #[error("décodage audio impossible : {0}")]
    Decode(String),
    #[error("analyse impossible : {0}")]
    Analysis(String),
}
