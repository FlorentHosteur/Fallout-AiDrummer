//! Lecture du projet REAPER : grille (tempo, mesures) et son des pistes sur la sélection de temps.

use aidrummer_engine::{
    accent_map, detect_onsets_after_preroll, Audio, Grid, ProjectSpan, SelectionAnalysis, TrackAccents,
};
use reaper_medium::{
    MediaTrack, PositionInSeconds, ProjectContext, Reaper, TimeRangeType,
};

const PROJECT: ProjectContext = ProjectContext::CurrentProject;
/// Fréquence à laquelle on demande le son des pistes. Suffisant pour dater des attaques.
const SAMPLE_RATE: u32 = 44_100;
/// Doubles croches.
const SLOTS_PER_BEAT: u32 = 4;
const MIN_SELECTION_S: f64 = 0.5;
/// Avance de lecture avant la sélection.
const PREROLL_S: f64 = 0.1;
const MAX_SELECTION_S: f64 = 15.0 * 60.0;
/// En dessous de ce niveau de crête, une piste est tenue pour silencieuse.
const SILENCE_PEAK: f32 = 1e-4;

/// Analyse la sélection de temps du projet ouvert : carte d'accents de chaque piste qui sonne.
pub fn analyze_selection(reaper: &Reaper) -> Result<SelectionAnalysis, String> {
    let selection = reaper
        .get_set_loop_time_range_2_get(PROJECT, TimeRangeType::TimeSelection)
        .map(|range| (range.start.get(), range.end.get()))
        .filter(|(start, end)| end - start >= MIN_SELECTION_S)
        .ok_or("Fais d'abord une sélection de temps sur le passage à analyser (au moins une demi-seconde).")?;
    let (start_s, end_s) = selection;
    if end_s - start_s > MAX_SELECTION_S {
        return Err("La sélection dépasse 15 minutes. Réduis-la à un passage.".to_string());
    }

    let (grid, tempo_changes) = read_grid(reaper, start_s, end_s)?;

    // On lit un peu avant la sélection, pour ne pas prendre une note qui résonne déjà
    // pour une attaque (voir `detect_onsets_after_preroll`).
    let preroll_s = PREROLL_S.min(start_s);

    let mut tracks = Vec::new();
    let mut silent_tracks = Vec::new();
    for index in 0..reaper.count_tracks(PROJECT) {
        let Some(track) = reaper.get_track(PROJECT, index) else {
            continue;
        };
        let name = track_name(reaper, track, index);
        match read_track_audio(reaper, track, start_s - preroll_s, end_s) {
            Some(audio) => {
                let (passage, onsets) = detect_onsets_after_preroll(&audio, preroll_s);
                tracks.push(TrackAccents {
                    name,
                    accents: accent_map(&passage, &onsets, grid),
                });
            }
            None => silent_tracks.push(name),
        }
    }

    Ok(SelectionAnalysis {
        start_s,
        end_s,
        grid,
        tempo_changes,
        tracks,
        silent_tracks,
    })
}

/// Lit la grille du projet autour de la sélection, et dit si le tempo y change.
fn read_grid(reaper: &Reaper, start_s: f64, end_s: f64) -> Result<(Grid, bool), String> {
    let qn_at = |seconds: f64| -> Result<f64, String> {
        let position = PositionInSeconds::new(seconds).map_err(|e| e.to_string())?;
        Ok(reaper.time_map_2_time_to_qn(PROJECT, position).get())
    };
    let start = PositionInSeconds::new(start_s).map_err(|e| e.to_string())?;
    let measure_index = reaper.time_map_2_time_to_beats(PROJECT, start).measure_index;
    let measure = reaper.time_map_get_measure_info(PROJECT, measure_index);

    let (start_qn, end_qn) = (qn_at(start_s)?, qn_at(end_s)?);
    let grid = Grid::from_project(
        ProjectSpan {
            start_s,
            end_s,
            start_qn,
            end_qn,
            bar_start_s: measure.start_time.get(),
            bar_length_qn: measure.end_qn.get() - measure.start_qn.get(),
        },
        SLOTS_PER_BEAT,
    )
    .ok_or("Impossible de lire la grille du projet sur cette sélection.")?;

    // À tempo constant, le milieu de la sélection tombe au milieu en noires aussi.
    let middle_qn = qn_at((start_s + end_s) / 2.0)?;
    let tempo_changes = (middle_qn - (start_qn + end_qn) / 2.0).abs() > 0.02;
    Ok((grid, tempo_changes))
}

fn track_name(reaper: &Reaper, track: MediaTrack, index: u32) -> String {
    let name = unsafe {
        reaper.get_set_media_track_info_get_name(track, |name| name.to_str().trim().to_owned())
    }
    .unwrap_or_default();
    if name.is_empty() {
        format!("Piste {}", index + 1)
    } else {
        name
    }
}

/// Lit en mono le son des items de la piste (avant ses effets) entre `start_s` et `end_s`.
/// Renvoie `None` si la piste ne sonne pas dans cet intervalle.
fn read_track_audio(reaper: &Reaper, track: MediaTrack, start_s: f64, end_s: f64) -> Option<Audio> {
    let low = reaper.low();
    let total = ((end_s - start_s) * SAMPLE_RATE as f64) as usize;
    let chunk = SAMPLE_RATE as usize;
    let mut samples = vec![0.0f32; total];
    let mut buffer = vec![0.0f64; chunk];
    let mut peak = 0.0f32;

    // L'accesseur est détruit avant de sortir, quel que soit le résultat de la lecture.
    let accessor = unsafe { low.CreateTrackAudioAccessor(track.as_ptr()) };
    if accessor.is_null() {
        return None;
    }
    let mut done = 0;
    while done < total {
        let count = chunk.min(total - done);
        let status = unsafe {
            low.GetAudioAccessorSamples(
                accessor,
                SAMPLE_RATE as i32,
                1,
                start_s + done as f64 / SAMPLE_RATE as f64,
                count as i32,
                buffer.as_mut_ptr(),
            )
        };
        // 1 : du son a été écrit ; 0 : rien à cet endroit ; -1 : erreur.
        if status == 1 {
            for (target, &source) in samples[done..done + count].iter_mut().zip(&buffer[..count]) {
                *target = source as f32;
                peak = peak.max(target.abs());
            }
        }
        done += count;
    }
    unsafe { low.DestroyAudioAccessor(accessor) };

    (peak > SILENCE_PEAK).then_some(Audio {
        samples,
        sample_rate: SAMPLE_RATE,
    })
}
