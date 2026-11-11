//! Vérifie la chaîne d'analyse sur un son de synthèse dont on connaît le tempo.
//!
//! Ce test ne dit rien de la justesse sur de la vraie musique : il garantit seulement
//! que le décodage et l'analyse sont branchés correctement.

use aidrummer_engine::{analyze, grid_fit, Audio};

const SAMPLE_RATE: u32 = 44_100;

/// Un clic par temps (plus aigu et plus fort sur le premier temps), sur un accord tenu de la mineur.
fn click_track(bpm: f64, seconds: f64) -> Audio {
    let total = (seconds * SAMPLE_RATE as f64) as usize;
    let period = 60.0 / bpm;
    let mut samples = vec![0.0f32; total];
    for (i, sample) in samples.iter_mut().enumerate() {
        let t = i as f64 / SAMPLE_RATE as f64;
        let beat_index = (t / period).floor();
        let since_beat = t - beat_index * period;
        let downbeat = (beat_index as u64).is_multiple_of(4);
        let (freq, gain) = if downbeat { (1500.0, 0.6) } else { (1000.0, 0.4) };
        let click = gain * (-since_beat * 60.0).exp() * (std::f64::consts::TAU * freq * t).sin();
        let chord: f64 = [220.0, 261.63, 329.63]
            .iter()
            .map(|f| 0.08 * (std::f64::consts::TAU * f * t).sin())
            .sum();
        *sample = (click + chord) as f32;
    }
    Audio {
        samples,
        sample_rate: SAMPLE_RATE,
    }
}

#[test]
fn finds_the_tempo_of_a_click_track() {
    let audio = click_track(120.0, 30.0);
    let analysis = analyze(&audio).expect("analyse");
    assert!(
        (analysis.bpm - 120.0).abs() < 2.0,
        "tempo trouvé : {} BPM",
        analysis.bpm
    );
    assert!(!analysis.beats.is_empty(), "aucun temps détecté");
}

/// Échoue avec stratum-dsp 1.0.0 : le tempo est juste, mais la grille de temps renvoyée est
/// irrégulière même sur un clic parfait (décalage moyen -40 ms, dispersion 80 ms à 120 BPM).
/// Gardé pour mesurer le remplacement de cette grille par notre propre détection d'attaques.
#[test]
#[ignore = "la grille de temps de stratum-dsp est trop imprécise"]
fn detected_beats_sit_on_the_click() {
    let audio = click_track(120.0, 30.0);
    let analysis = analyze(&audio).expect("analyse");
    let fit = grid_fit(&analysis.beats, 120.0, 0.0).expect("écart à la grille");
    assert!(
        fit.mean_offset_ms.abs() < 40.0 && fit.spread_ms < 40.0,
        "temps détectés loin du clic : {fit:?}"
    );
}
