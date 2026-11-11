//! Lecture et écriture du projet REAPER : grille (tempo, mesures), son des pistes et batterie
//! MIDI sur la sélection de temps.

use std::ffi::{c_char, CStr, CString};
use std::ptr::null_mut;

use aidrummer_engine::{
    accent_map, detect_onsets_after_preroll, read_drums, span_label, template_accents, write_drums,
    AccentMap, Audio, DrumNote, DrumPlan, Grid, ProjectSpan, SelectionAnalysis, TrackAccents,
};
use reaper_medium::{MediaTrack, PositionInSeconds, ProjectContext, Reaper, TimeRangeType};

const PROJECT: ProjectContext = ProjectContext::CurrentProject;
/// Fréquence à laquelle on demande le son des pistes. Suffisant pour dater des attaques.
const SAMPLE_RATE: u32 = 44_100;
/// Doubles croches.
const SLOTS_PER_BEAT: u32 = 4;
const MIN_SELECTION_S: f64 = 0.5;
/// Marge pour reconnaître une sélection posée sur une barre de mesure.
const BAR_EDGE_S: f64 = 0.002;
/// Avance de lecture avant la sélection.
const PREROLL_S: f64 = 0.1;
const MAX_SELECTION_S: f64 = 15.0 * 60.0;
/// En dessous de ce niveau de crête, une piste est tenue pour silencieuse.
const SILENCE_PEAK: f32 = 1e-4;
/// Piste sur laquelle l'extension écrit ses ébauches.
pub const SKETCH_TRACK: &str = "AI Drummer";
/// Durée des notes écrites, en noires. Une batterie ne tient pas ses notes.
const NOTE_LENGTH_QN: f64 = 0.125;
/// Canal MIDI des notes écrites (0 = canal 1). Les batteries virtuelles écoutent tous les canaux.
const NOTE_CHANNEL: i32 = 0;
/// Tolérance sur les bords de la sélection, en noires.
const EDGE_QN: f64 = 1e-6;

/// La grille du projet autour de la sélection, avec ses repères en noires.
struct ProjectGrid {
    grid: Grid,
    /// Numéro, dans le projet, de la mesure qui contient le début de la sélection.
    first_measure: i64,
    tempo_changes: bool,
    /// Début et fin de la sélection, en noires depuis le début du projet.
    start_qn: f64,
    end_qn: f64,
    /// Début, en noires, de la mesure qui contient le début de la sélection (mesure 0 de la grille).
    origin_qn: f64,
}

/// Analyse la sélection de temps du projet ouvert : carte d'accents de chaque piste qui sonne,
/// et lecture des parties de batterie MIDI.
pub fn analyze_selection(reaper: &Reaper) -> Result<SelectionAnalysis, String> {
    let (start_s, end_s) = selection(reaper)?;
    let project_grid = read_grid(reaper, start_s, end_s)?;
    Ok(analyze(reaper, start_s, end_s, &project_grid).0)
}

/// Un passage analysé, prêt à recevoir une batterie. Ne contient que des données : il peut
/// attendre la réponse de l'IA hors du fil principal.
pub struct Passage {
    pub analysis: SelectionAnalysis,
    /// Les parties de batterie MIDI du passage : (nom de la piste, notes).
    pub drums: Vec<(String, Vec<DrumNote>)>,
    grid: ProjectGrid,
}

impl Passage {
    pub fn grid(&self) -> &Grid {
        &self.grid.grid
    }

    /// Les cartes d'accents des pistes audio du passage. Vide si rien ne sonne.
    pub fn audio_accents(&self) -> Vec<AccentMap> {
        self.analysis
            .tracks
            .iter()
            .map(|t| t.accents.clone())
            .collect()
    }

    /// Ce sur quoi caler une batterie : les accents des pistes audio, ou un motif de base
    /// quand rien ne sonne dans la sélection (composition à partir de zéro).
    pub fn accents(&self) -> Vec<AccentMap> {
        if self.is_from_scratch() {
            let duration_s = self.analysis.end_s - self.analysis.start_s;
            vec![template_accents(self.grid.grid, duration_s)]
        } else {
            self.audio_accents()
        }
    }

    pub fn is_from_scratch(&self) -> bool {
        self.analysis.tracks.is_empty()
    }
}

/// Analyse la sélection : accents des pistes qui sonnent et batteries MIDI existantes.
pub fn prepare_passage(reaper: &Reaper) -> Result<Passage, String> {
    let (start_s, end_s) = selection(reaper)?;
    let grid = read_grid(reaper, start_s, end_s)?;
    let (analysis, drums) = analyze(reaper, start_s, end_s, &grid);
    Ok(Passage {
        analysis,
        drums,
        grid,
    })
}

/// Écrit une batterie sur le passage, selon `plan`, dans un nouvel item MIDI de la piste
/// « AI Drummer » (créée au besoin). Renvoie un message pour la console.
pub fn write_passage(
    reaper: &Reaper,
    passage: &Passage,
    plan: &DrumPlan,
) -> Result<String, String> {
    let part = write_drums(passage.grid(), &passage.accents(), plan);
    let written = write_notes(reaper, passage, &part)?;
    let basis = if passage.is_from_scratch() {
        "posées sur un motif de base, faute de son dans la sélection".to_string()
    } else {
        format!(
            "calées sur : {}",
            passage
                .analysis
                .tracks
                .iter()
                .map(|t| t.name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        )
    };
    Ok(format!(
        "{written} notes écrites sur la piste « {SKETCH_TRACK} », {}, {basis}.\n  Pour annuler : Édition → Annuler.",
        span_label(&passage.analysis),
    ))
}

/// Écrit des notes sur le passage, dans un nouvel item MIDI de la piste « AI Drummer ».
/// Les notes hors de la sélection sont laissées de côté. Renvoie le nombre de notes écrites.
pub fn write_notes(reaper: &Reaper, passage: &Passage, part: &[DrumNote]) -> Result<usize, String> {
    let (start_s, end_s) = (passage.analysis.start_s, passage.analysis.end_s);
    let grid = &passage.grid;
    let part: Vec<DrumNote> = part
        .iter()
        .copied()
        .filter(|note| {
            let qn = grid.origin_qn + note.position_qn;
            qn >= grid.start_qn - EDGE_QN && qn < grid.end_qn - EDGE_QN
        })
        .collect();
    if part.is_empty() {
        return Err("La batterie ne contient aucune note sur cette sélection.".to_string());
    }

    let low = reaper.low();
    unsafe { low.Undo_BeginBlock2(null_mut()) };
    let written = write_part(reaper, &part, start_s, end_s, grid.origin_qn);
    unsafe { low.Undo_EndBlock2(null_mut(), c"AI Drummer : batterie".as_ptr(), -1) };
    low.UpdateArrange();
    written?;
    Ok(part.len())
}

fn selection(reaper: &Reaper) -> Result<(f64, f64), String> {
    let (start_s, end_s) = reaper
        .get_set_loop_time_range_2_get(PROJECT, TimeRangeType::TimeSelection)
        .map(|range| (range.start.get(), range.end.get()))
        .filter(|(start, end)| end - start >= MIN_SELECTION_S)
        .ok_or("Fais d'abord une sélection de temps sur le passage (au moins une demi-seconde).")?;
    if end_s - start_s > MAX_SELECTION_S {
        return Err("La sélection dépasse 15 minutes. Réduis-la à un passage.".to_string());
    }
    Ok((start_s, end_s))
}

fn analyze(
    reaper: &Reaper,
    start_s: f64,
    end_s: f64,
    project_grid: &ProjectGrid,
) -> (SelectionAnalysis, Vec<(String, Vec<DrumNote>)>) {
    let grid = project_grid.grid;
    // On lit un peu avant la sélection, pour ne pas prendre une note qui résonne déjà
    // pour une attaque (voir `detect_onsets_after_preroll`).
    let preroll_s = PREROLL_S.min(start_s);

    let mut tracks = Vec::new();
    let mut silent_tracks = Vec::new();
    let mut drum_parts = Vec::new();
    for index in 0..reaper.count_tracks(PROJECT) {
        let Some(track) = reaper.get_track(PROJECT, index) else {
            continue;
        };
        let name = track_name(reaper, track, index);
        if is_drum_track(reaper, track) {
            let part = read_midi_notes(reaper, track, project_grid);
            if !part.is_empty() {
                drum_parts.push((name.clone(), part));
            }
        }
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

    let maps: Vec<AccentMap> = tracks.iter().map(|t| t.accents.clone()).collect();
    let drums = drum_parts
        .iter()
        .map(|(name, part)| read_drums(name, part, &grid, &maps))
        .collect();

    let analysis = SelectionAnalysis {
        start_s,
        end_s,
        first_measure: project_grid.first_measure,
        grid,
        tempo_changes: project_grid.tempo_changes,
        tracks,
        silent_tracks,
        drums,
    };
    (analysis, drum_parts)
}

/// Lit la grille du projet autour de la sélection, et dit si le tempo y change.
fn read_grid(reaper: &Reaper, start_s: f64, end_s: f64) -> Result<ProjectGrid, String> {
    let qn_at = |seconds: f64| -> Result<f64, String> {
        let position = PositionInSeconds::new(seconds).map_err(|e| e.to_string())?;
        Ok(reaper.time_map_2_time_to_qn(PROJECT, position).get())
    };
    // Une sélection posée sur une barre de mesure peut tomber un cheveu avant elle (arrondi) :
    // on regarde un rien plus loin pour ne pas compter la mesure précédente.
    let start = PositionInSeconds::new(start_s + BAR_EDGE_S).map_err(|e| e.to_string())?;
    let measure_index = reaper
        .time_map_2_time_to_beats(PROJECT, start)
        .measure_index;
    let measure = reaper.time_map_get_measure_info(PROJECT, measure_index);

    let (start_qn, end_qn) = (qn_at(start_s)?, qn_at(end_s)?);
    let origin_qn = measure.start_qn.get();
    // Sélection posée sur la barre : la mesure commence exactement avec elle.
    let bar_start_s = if (measure.start_time.get() - start_s).abs() < BAR_EDGE_S {
        start_s
    } else {
        measure.start_time.get()
    };
    let grid = Grid::from_project(
        ProjectSpan {
            start_s,
            end_s,
            start_qn,
            end_qn,
            bar_start_s,
            bar_length_qn: measure.end_qn.get() - origin_qn,
        },
        SLOTS_PER_BEAT,
    )
    .ok_or("Impossible de lire la grille du projet sur cette sélection.")?;

    // À tempo constant, le milieu de la sélection tombe au milieu en noires aussi.
    let middle_qn = qn_at((start_s + end_s) / 2.0)?;
    let tempo_changes = (middle_qn - (start_qn + end_qn) / 2.0).abs() > 0.02;
    Ok(ProjectGrid {
        grid,
        // REAPER compte les mesures à partir de 0 ; il les affiche à partir de 1.
        first_measure: measure_index as i64 + 1,
        tempo_changes,
        start_qn,
        end_qn,
        origin_qn,
    })
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

/// Une piste est tenue pour une batterie si son nom ou l'un de ses effets le dit
/// (« drums », « Batterie », « Superior Drummer », « MT Power Drum Kit »…), ou si c'est le cas
/// d'un dossier qui la contient : l'instrument est souvent posé sur le dossier, et le MIDI
/// dans ses pistes.
fn is_drum_track(reaper: &Reaper, track: MediaTrack) -> bool {
    let low = reaper.low();
    let says_drums = |text: &str| {
        let text = text.to_lowercase();
        ["drum", "batterie", "kit"]
            .iter()
            .any(|word| text.contains(word))
    };
    let mut buffer = [0 as c_char; 256];
    let mut current = track.as_ptr();
    while !current.is_null() {
        let named = unsafe { low.GetTrackName(current, buffer.as_mut_ptr(), buffer.len() as i32) };
        if named && says_drums(&unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy()) {
            return true;
        }
        let effects = unsafe { low.TrackFX_GetCount(current) };
        for fx in 0..effects {
            let found = unsafe {
                low.TrackFX_GetFXName(current, fx, buffer.as_mut_ptr(), buffer.len() as i32)
            };
            if found && says_drums(&unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_string_lossy()) {
                return true;
            }
        }
        current = unsafe { low.GetParentTrack(current) };
    }
    false
}

/// Lit les notes MIDI de la piste qui commencent dans la sélection. Les items et les notes
/// muets sont ignorés.
fn read_midi_notes(
    reaper: &Reaper,
    track: MediaTrack,
    project_grid: &ProjectGrid,
) -> Vec<DrumNote> {
    let low = reaper.low();
    let mut part = Vec::new();
    let items = unsafe { low.CountTrackMediaItems(track.as_ptr()) };
    for item_index in 0..items {
        let item = unsafe { low.GetTrackMediaItem(track.as_ptr(), item_index) };
        if item.is_null() || unsafe { low.GetMediaItemInfo_Value(item, c"B_MUTE".as_ptr()) } != 0.0
        {
            continue;
        }
        let take = unsafe { low.GetActiveTake(item) };
        if take.is_null() || !unsafe { low.TakeIsMIDI(take) } {
            continue;
        }
        let mut note_count = 0;
        unsafe { low.MIDI_CountEvts(take, &mut note_count, null_mut(), null_mut()) };
        for note_index in 0..note_count {
            let (mut selected, mut muted) = (false, false);
            let (mut start_ppq, mut end_ppq) = (0.0, 0.0);
            let (mut channel, mut pitch, mut velocity) = (0, 0, 0);
            let found = unsafe {
                low.MIDI_GetNote(
                    take,
                    note_index,
                    &mut selected,
                    &mut muted,
                    &mut start_ppq,
                    &mut end_ppq,
                    &mut channel,
                    &mut pitch,
                    &mut velocity,
                )
            };
            if !found || muted {
                continue;
            }
            let qn = unsafe { low.MIDI_GetProjQNFromPPQPos(take, start_ppq) };
            if qn >= project_grid.start_qn - EDGE_QN && qn < project_grid.end_qn - EDGE_QN {
                part.push(DrumNote {
                    position_qn: qn - project_grid.origin_qn,
                    note: pitch.clamp(0, 127) as u8,
                    velocity: velocity.clamp(1, 127) as u8,
                });
            }
        }
    }
    part
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

/// Trouve la piste « AI Drummer », ou la crée en bas du projet.
fn sketch_track(reaper: &Reaper) -> Result<MediaTrack, String> {
    let count = reaper.count_tracks(PROJECT);
    for index in 0..count {
        if let Some(track) = reaper.get_track(PROJECT, index) {
            if track_name(reaper, track, index) == SKETCH_TRACK {
                return Ok(track);
            }
        }
    }
    let low = reaper.low();
    low.InsertTrackAtIndex(count as i32, true);
    let track = reaper
        .get_track(PROJECT, count)
        .ok_or("Création de la piste « AI Drummer » impossible.")?;
    let name = CString::new(SKETCH_TRACK).map_err(|e| e.to_string())?;
    unsafe {
        low.GetSetMediaTrackInfo_String(
            track.as_ptr(),
            c"P_NAME".as_ptr(),
            name.as_ptr() as *mut c_char,
            true,
        )
    };
    Ok(track)
}

/// Crée un item MIDI sur la sélection et y écrit les notes.
fn write_part(
    reaper: &Reaper,
    part: &[DrumNote],
    start_s: f64,
    end_s: f64,
    origin_qn: f64,
) -> Result<(), String> {
    let low = reaper.low();
    let track = sketch_track(reaper)?;
    let item = unsafe { low.CreateNewMIDIItemInProj(track.as_ptr(), start_s, end_s, &false) };
    if item.is_null() {
        return Err("Création de l'item MIDI impossible.".to_string());
    }
    let take = unsafe { low.GetActiveTake(item) };
    if take.is_null() {
        return Err("L'item MIDI créé n'a pas de prise.".to_string());
    }
    for note in part {
        let qn = origin_qn + note.position_qn;
        unsafe {
            let start_ppq = low.MIDI_GetPPQPosFromProjQN(take, qn);
            let end_ppq = low.MIDI_GetPPQPosFromProjQN(take, qn + NOTE_LENGTH_QN);
            low.MIDI_InsertNote(
                take,
                false,
                false,
                start_ppq,
                end_ppq,
                NOTE_CHANNEL,
                note.note as i32,
                note.velocity as i32,
                // Tri différé : un seul tri à la fin, plus rapide.
                &true,
            );
        }
    }
    unsafe { low.MIDI_Sort(take) };
    Ok(())
}
