//! Analyse d'un extrait : tempo, tonalité, temps, et écart du jeu par rapport à la grille du projet.

use serde::Serialize;
use stratum_dsp::{analyze_audio, AnalysisConfig};

use crate::{Audio, EngineError};

/// Ce que l'analyse audio trouve seule, sans rien savoir du projet Reaper.
#[derive(Debug, Clone, Serialize)]
pub struct SongAnalysis {
    pub duration_s: f64,
    pub bpm: f32,
    /// Entre 0 et 1.
    pub bpm_confidence: f32,
    /// Par exemple « A minor ».
    pub key: String,
    /// Entre 0 et 1.
    pub key_confidence: f32,
    /// Régularité de la grille de temps détectée, entre 0 et 1.
    pub grid_stability: f32,
    /// Position des temps, en secondes depuis le début de l'extrait.
    pub beats: Vec<f32>,
    /// Position des premiers temps de mesure, en secondes.
    pub downbeats: Vec<f32>,
    pub warnings: Vec<String>,
}

pub fn analyze(audio: &Audio) -> Result<SongAnalysis, EngineError> {
    let result = analyze_audio(&audio.samples, audio.sample_rate, AnalysisConfig::default())
        .map_err(|e| EngineError::Analysis(e.to_string()))?;
    Ok(SongAnalysis {
        duration_s: audio.duration_s(),
        bpm: result.bpm,
        bpm_confidence: result.bpm_confidence,
        key: result.key.name(),
        key_confidence: result.key_confidence,
        grid_stability: result.grid_stability,
        beats: result.beat_grid.beats,
        downbeats: result.beat_grid.downbeats,
        warnings: result.metadata.confidence_warnings,
    })
}

/// Écart entre les temps détectés et la grille du projet.
///
/// Sert au mode composition : le projet a un tempo, mais le jeu n'est pas forcément calé dessus.
#[derive(Debug, Clone, Serialize)]
pub struct GridFit {
    pub project_bpm: f64,
    /// Décalage moyen du jeu par rapport à la grille, en millisecondes.
    /// Positif : le jeu est en retard sur le clic.
    pub mean_offset_ms: f64,
    /// Dispersion autour de ce décalage, en millisecondes. Petit : jeu régulier.
    pub spread_ms: f64,
    pub beats_compared: usize,
}

/// Compare des temps (en secondes) à une grille régulière de tempo `project_bpm`
/// dont un temps tombe à `grid_origin_s`.
///
/// Les écarts sont mesurés sur le cercle d'un temps : un temps détecté juste avant
/// le clic compte comme une petite avance, pas comme un grand retard.
pub fn grid_fit(beats: &[f64], project_bpm: f64, grid_origin_s: f64) -> Option<GridFit> {
    if beats.is_empty() || project_bpm <= 0.0 {
        return None;
    }
    let period = 60.0 / project_bpm;
    let (mut sin_sum, mut cos_sum) = (0.0f64, 0.0f64);
    for &beat in beats {
        let phase = ((beat - grid_origin_s) / period).rem_euclid(1.0) * std::f64::consts::TAU;
        sin_sum += phase.sin();
        cos_sum += phase.cos();
    }
    let n = beats.len() as f64;
    let mean_angle = sin_sum.atan2(cos_sum);
    // Longueur du vecteur moyen : 1 si tous les temps ont le même décalage, 0 s'ils sont répartis au hasard.
    let resultant = (sin_sum.powi(2) + cos_sum.powi(2)).sqrt() / n;
    // `abs` : un jeu parfaitement régulier donne -0.0, qui s'afficherait « -0 ms ».
    let spread_angle = (-2.0 * resultant.clamp(1e-12, 1.0).ln()).abs().sqrt();
    let to_ms = period / std::f64::consts::TAU * 1000.0;
    Some(GridFit {
        project_bpm,
        mean_offset_ms: mean_angle * to_ms,
        spread_ms: spread_angle * to_ms,
        beats_compared: beats.len(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_fit_measures_a_constant_delay() {
        // Temps à 120 BPM, tous en retard de 20 ms.
        let beats: Vec<f64> = (0..32).map(|i| i as f64 * 0.5 + 0.020).collect();
        let fit = grid_fit(&beats, 120.0, 0.0).unwrap();
        assert!((fit.mean_offset_ms - 20.0).abs() < 0.5, "{fit:?}");
        assert!(fit.spread_ms < 0.5, "{fit:?}");
    }

    #[test]
    fn grid_fit_reads_an_early_beat_as_early() {
        // 10 ms d'avance : ne doit pas être lu comme 490 ms de retard.
        let beats: Vec<f64> = (1..32).map(|i| i as f64 * 0.5 - 0.010).collect();
        let fit = grid_fit(&beats, 120.0, 0.0).unwrap();
        assert!((fit.mean_offset_ms + 10.0).abs() < 0.5, "{fit:?}");
    }

    #[test]
    fn grid_fit_spread_grows_with_loose_playing() {
        let tight: Vec<f64> = (0..64).map(|i| i as f64 * 0.5).collect();
        let loose: Vec<f64> = (0..64)
            .map(|i| i as f64 * 0.5 + if i % 2 == 0 { 0.030 } else { -0.030 })
            .collect();
        let tight_fit = grid_fit(&tight, 120.0, 0.0).unwrap();
        let loose_fit = grid_fit(&loose, 120.0, 0.0).unwrap();
        assert!(loose_fit.spread_ms > tight_fit.spread_ms + 20.0);
    }

    #[test]
    fn grid_fit_needs_beats_and_a_tempo() {
        assert!(grid_fit(&[], 120.0, 0.0).is_none());
        assert!(grid_fit(&[1.0], 0.0, 0.0).is_none());
    }
}
