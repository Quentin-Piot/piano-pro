use ndarray::{Array2, Array3, ArrayView1, ArrayView2, Axis, concatenate, s};
use rten::{NodeId, ValueOrView};
use rten_tensor::{prelude::*, *};

const FRAMES_PER_SECOND: f32 = 100.0;
const REGRESSION_SAMPLE_RATE: u32 = 16000;
const REGRESSION_SEGMENT_SAMPLES: usize = REGRESSION_SAMPLE_RATE as usize * 10;

const BASIC_PITCH_SAMPLE_RATE: u32 = 22050;
const BASIC_PITCH_FFT_HOP: usize = 256;
const BASIC_PITCH_AUDIO_N_SAMPLES: usize = 43844;
const BASIC_PITCH_ANNOT_N_FRAMES: usize = 172;
const BASIC_PITCH_DEFAULT_OVERLAPPING_FRAMES: usize = 30;
const BASIC_PITCH_ONSET_THRESHOLD: f32 = 0.5;
const BASIC_PITCH_FRAME_THRESHOLD: f32 = 0.3;
const BASIC_PITCH_MIN_NOTE_LEN: usize = 11;
const BASIC_PITCH_MIDI_OFFSET: usize = 21;
const BASIC_PITCH_MAX_FREQ_IDX: usize = 87;
const BASIC_PITCH_MAGIC_ALIGNMENT_OFFSET: f32 = 0.0018;
const DEFAULT_MODEL: &[u8] = include_bytes!("../assets/models/basic-pitch.rten");

mod audio;

pub fn transcribe_audio_to_midi(
    input: impl AsRef<std::path::Path>,
) -> anyhow::Result<midly::Smf<'static>> {
    let model = rten::Model::load_static_slice(DEFAULT_MODEL)?;
    transcribe_audio_to_midi_with_loaded_model(input.as_ref(), &model)
}

pub fn transcribe_audio_to_midi_with_model(
    input: impl AsRef<std::path::Path>,
    model_path: impl AsRef<std::path::Path>,
) -> anyhow::Result<midly::Smf<'static>> {
    let model = rten::Model::load_file(model_path)?;
    transcribe_audio_to_midi_with_loaded_model(input.as_ref(), &model)
}

fn transcribe_audio_to_midi_with_loaded_model(
    input: &std::path::Path,
    model: &rten::Model,
) -> anyhow::Result<midly::Smf<'static>> {
    match model.output_ids().len() {
        3 => run_basic_pitch_model(input, model),
        7 => run_regression_model(input, model),
        count => anyhow::bail!("unsupported model signature: expected 3 or 7 outputs, got {count}"),
    }
}

fn run_regression_model(
    input_path: &std::path::Path,
    model: &rten::Model,
) -> anyhow::Result<midly::Smf<'static>> {
    let input = audio::load(
        input_path,
        REGRESSION_SAMPLE_RATE,
        Some(REGRESSION_SEGMENT_SAMPLES),
    )?;

    let input = ArrayView2::from_shape([1, input.len()], &input)?;
    let input = enframe(&input, REGRESSION_SEGMENT_SAMPLES);
    let input = input.as_slice().unwrap().to_vec();

    let input = Tensor::from_data(
        &[
            input.len() / REGRESSION_SEGMENT_SAMPLES,
            REGRESSION_SEGMENT_SAMPLES,
        ],
        input,
    );

    let inputs: Vec<(NodeId, ValueOrView)> = vec![(model.input_ids()[0], input.view().into())];

    let [
        reg_onset_output,
        reg_offset_output,
        frame_output,
        _velocity_output,
        _reg_pedal_onset_output,
        _reg_pedal_offset_output,
        _pedal_frame_output,
    ] = model.run_n::<7>(inputs, model.output_ids().try_into()?, None)?;

    let (onset_output, onset_shift_output) = {
        let output = reg_onset_output.into_tensor::<f32>().unwrap();
        let shape: [usize; 3] = output.shape().try_into().unwrap();
        let reg_onset_output = Array3::from_shape_vec(shape, output.to_vec()).unwrap();
        let reg_onset_output: Array2<_> = deframe(&reg_onset_output);

        let onset_threshold = 0.3;
        get_binarized_output_from_regression(&reg_onset_output.view(), onset_threshold, 2)
    };

    let (offset_output, offset_shift_output) = {
        let output = reg_offset_output.into_tensor::<f32>().unwrap();
        let shape: [usize; 3] = output.shape().try_into().unwrap();
        let reg_offset_output: Array3<_> = Array3::from_shape_vec(shape, output.to_vec()).unwrap();
        let reg_offset_output: Array2<_> = deframe(&reg_offset_output);

        let offset_threshold = 0.2;
        get_binarized_output_from_regression(&reg_offset_output.view(), offset_threshold, 4)
    };

    let frame_output: Array3<_> = {
        let output = frame_output.into_tensor::<f32>().unwrap();
        let shape: [usize; 3] = output.shape().try_into().unwrap();
        Array3::from_shape_vec(shape, output.to_vec()).unwrap()
    };
    let frame_output: Array2<_> = deframe(&frame_output);

    let frame_threshold = 0.1;

    Ok(note_detection_with_onset_offset_regress(
        frame_output.view(),
        onset_output.view(),
        onset_shift_output.view(),
        offset_output.view(),
        offset_shift_output.view(),
        (), // velocity_output,
        frame_threshold,
    ))
}

fn run_basic_pitch_model(
    input_path: &std::path::Path,
    model: &rten::Model,
) -> anyhow::Result<midly::Smf<'static>> {
    let input = audio::load(input_path, BASIC_PITCH_SAMPLE_RATE, None)?;
    let original_len = input.len();
    let input = basic_pitch_window_audio(&input);
    let n_windows = input.len() / BASIC_PITCH_AUDIO_N_SAMPLES;
    let input = Tensor::from_data(&[n_windows, BASIC_PITCH_AUDIO_N_SAMPLES, 1], input);

    let inputs: Vec<(NodeId, ValueOrView)> = vec![(model.input_ids()[0], input.view().into())];
    let outputs = model.run(inputs, model.output_ids(), None)?;

    let mut note_output = None;
    let mut onset_output = None;
    let mut contour_output = None;

    // The bundled Basic Pitch model names its outputs with a trailing ":<index>"
    // suffix where :0 = contour, :1 = note (frame), :2 = onset.
    // This matches the output order in the original basic-pitch ONNX export.
    for (id, output) in model.output_ids().iter().copied().zip(outputs) {
        let name = model
            .node_info(id)
            .and_then(|info| info.name())
            .unwrap_or_default();

        if name.ends_with(":1") {
            note_output = Some(output);
        } else if name.ends_with(":2") {
            onset_output = Some(output);
        } else if name.ends_with(":0") {
            contour_output = Some(output);
        }
    }

    let note_output =
        note_output.ok_or_else(|| anyhow::anyhow!("missing Basic Pitch note output"))?;
    let onset_output =
        onset_output.ok_or_else(|| anyhow::anyhow!("missing Basic Pitch onset output"))?;
    let contour_output =
        contour_output.ok_or_else(|| anyhow::anyhow!("missing Basic Pitch contour output"))?;

    let note_output = basic_pitch_unwrap_output(note_output, original_len)?;
    let onset_output = basic_pitch_unwrap_output(onset_output, original_len)?;
    let _contour_output = basic_pitch_unwrap_output(contour_output, original_len)?;

    let note_events = basic_pitch_output_to_notes(note_output.view(), onset_output.view());
    let notes = note_events
        .into_iter()
        .map(|(bgn, fin, pitch, _amplitude)| {
            (
                pitch,
                basic_pitch_frame_to_time(bgn),
                basic_pitch_frame_to_time(fin),
            )
        })
        .collect();

    Ok(create_midi_file(notes))
}

fn basic_pitch_window_audio(input: &[f32]) -> Vec<f32> {
    let overlap_len = BASIC_PITCH_DEFAULT_OVERLAPPING_FRAMES * BASIC_PITCH_FFT_HOP;
    let hop_size = BASIC_PITCH_AUDIO_N_SAMPLES - overlap_len;

    let mut padded = vec![0.0; overlap_len / 2];
    padded.extend(input);

    let mut windows = Vec::new();
    for start in (0..padded.len()).step_by(hop_size) {
        let end = (start + BASIC_PITCH_AUDIO_N_SAMPLES).min(padded.len());
        windows.extend_from_slice(&padded[start..end]);
        windows.resize(
            windows.len() + BASIC_PITCH_AUDIO_N_SAMPLES - (end - start),
            0.0,
        );
    }

    windows
}

fn basic_pitch_unwrap_output(
    output: rten::Value,
    audio_original_length: usize,
) -> anyhow::Result<Array2<f32>> {
    let output = output
        .into_tensor::<f32>()
        .ok_or_else(|| anyhow::anyhow!("expected Basic Pitch output tensor"))?;
    let shape: [usize; 3] = output.shape().try_into()?;
    let output = Array3::from_shape_vec(shape, output.to_vec())?;

    let n_olap = BASIC_PITCH_DEFAULT_OVERLAPPING_FRAMES / 2;
    let output = output
        .slice(s![.., n_olap..(shape[1] - n_olap), ..])
        .to_owned();
    let (_, frames_per_window, bins) = output.dim();
    let mut output = output.into_shape_with_order((shape[0] * frames_per_window, bins))?;

    let hop_size =
        BASIC_PITCH_AUDIO_N_SAMPLES - BASIC_PITCH_DEFAULT_OVERLAPPING_FRAMES * BASIC_PITCH_FFT_HOP;
    let frames_per_window = BASIC_PITCH_ANNOT_N_FRAMES - BASIC_PITCH_DEFAULT_OVERLAPPING_FRAMES;
    let expected_frames = audio_original_length * frames_per_window / hop_size;
    if expected_frames < output.dim().0 {
        output = output.slice(s![0..expected_frames, ..]).to_owned();
    }

    Ok(output)
}

fn basic_pitch_output_to_notes(
    frames: ArrayView2<f32>,
    onsets: ArrayView2<f32>,
) -> Vec<(usize, usize, usize, f32)> {
    let onsets = basic_pitch_infer_onsets(onsets, frames);
    let mut remaining_energy = frames.to_owned();
    let mut notes = Vec::new();

    for (note_start_idx, freq_idx) in basic_pitch_onset_peaks(onsets.view()) {
        if note_start_idx >= frames.dim().0 - 1 {
            continue;
        }

        let mut i = note_start_idx + 1;
        let mut k = 0;
        while i < frames.dim().0 - 1 && k < 11 {
            if remaining_energy[[i, freq_idx]] < BASIC_PITCH_FRAME_THRESHOLD {
                k += 1;
            } else {
                k = 0;
            }
            i += 1;
        }

        i -= k;
        if i - note_start_idx <= BASIC_PITCH_MIN_NOTE_LEN {
            continue;
        }

        clear_basic_pitch_energy(&mut remaining_energy, note_start_idx, i, freq_idx);
        let amplitude = frames
            .slice(s![note_start_idx..i, freq_idx])
            .mean()
            .unwrap_or(0.0);
        notes.push((
            note_start_idx,
            i,
            freq_idx + BASIC_PITCH_MIDI_OFFSET,
            amplitude,
        ));
    }

    while let Some((i_mid, freq_idx)) = basic_pitch_max_energy(remaining_energy.view()) {
        if remaining_energy[[i_mid, freq_idx]] <= BASIC_PITCH_FRAME_THRESHOLD {
            break;
        }

        remaining_energy[[i_mid, freq_idx]] = 0.0;

        let mut i = i_mid + 1;
        let mut k = 0;
        while i < frames.dim().0 - 1 && k < 11 {
            if remaining_energy[[i, freq_idx]] < BASIC_PITCH_FRAME_THRESHOLD {
                k += 1;
            } else {
                k = 0;
            }
            clear_basic_pitch_energy(&mut remaining_energy, i, i + 1, freq_idx);
            i += 1;
        }
        let i_end = i.saturating_sub(1 + k);

        let mut i = i_mid.saturating_sub(1);
        let mut k = 0;
        while i > 0 && k < 11 {
            if remaining_energy[[i, freq_idx]] < BASIC_PITCH_FRAME_THRESHOLD {
                k += 1;
            } else {
                k = 0;
            }
            clear_basic_pitch_energy(&mut remaining_energy, i, i + 1, freq_idx);
            i -= 1;
        }
        let i_start = i + 1 + k;

        if i_end <= i_start || i_end - i_start <= BASIC_PITCH_MIN_NOTE_LEN {
            continue;
        }

        let amplitude = frames
            .slice(s![i_start..i_end, freq_idx])
            .mean()
            .unwrap_or(0.0);
        notes.push((
            i_start,
            i_end,
            freq_idx + BASIC_PITCH_MIDI_OFFSET,
            amplitude,
        ));
    }

    notes.sort_by_key(|note| note.0);
    notes
}

fn basic_pitch_infer_onsets(onsets: ArrayView2<f32>, frames: ArrayView2<f32>) -> Array2<f32> {
    let mut frame_diff = Array2::<f32>::zeros(frames.dim());

    for ((frame_id, freq_id), diff) in frame_diff.indexed_iter_mut() {
        let mut min_diff = f32::MAX;
        for n in 1..=2 {
            let prev = if frame_id >= n {
                frames[[frame_id - n, freq_id]]
            } else {
                0.0
            };
            min_diff = min_diff.min(frames[[frame_id, freq_id]] - prev);
        }

        *diff = min_diff.max(0.0);
    }

    frame_diff.slice_mut(s![0..2, ..]).fill(0.0);
    let max_diff = frame_diff.iter().copied().fold(0.0, f32::max);
    let max_onset = onsets.iter().copied().fold(0.0, f32::max);
    if max_diff > 0.0 {
        frame_diff.mapv_inplace(|value| max_onset * value / max_diff);
    }

    let mut inferred = onsets.to_owned();
    inferred.zip_mut_with(&frame_diff, |onset, diff| {
        *onset = onset.max(*diff);
    });
    inferred
}

fn basic_pitch_onset_peaks(onsets: ArrayView2<f32>) -> Vec<(usize, usize)> {
    let mut peaks = Vec::new();
    let (n_frames, n_freqs) = onsets.dim();

    for frame_id in 1..n_frames.saturating_sub(1) {
        for freq_id in 0..n_freqs {
            let onset = onsets[[frame_id, freq_id]];
            if onset >= BASIC_PITCH_ONSET_THRESHOLD
                && onset > onsets[[frame_id - 1, freq_id]]
                && onset > onsets[[frame_id + 1, freq_id]]
            {
                peaks.push((frame_id, freq_id));
            }
        }
    }

    peaks.sort_by(|a, b| b.cmp(a));
    peaks
}

fn clear_basic_pitch_energy(
    remaining_energy: &mut Array2<f32>,
    start: usize,
    end: usize,
    freq_idx: usize,
) {
    remaining_energy
        .slice_mut(s![start..end, freq_idx])
        .fill(0.0);
    if freq_idx < BASIC_PITCH_MAX_FREQ_IDX {
        remaining_energy
            .slice_mut(s![start..end, freq_idx + 1])
            .fill(0.0);
    }
    if freq_idx > 0 {
        remaining_energy
            .slice_mut(s![start..end, freq_idx - 1])
            .fill(0.0);
    }
}

fn basic_pitch_max_energy(remaining_energy: ArrayView2<f32>) -> Option<(usize, usize)> {
    remaining_energy
        .indexed_iter()
        .max_by(|(_, a), (_, b)| a.partial_cmp(b).unwrap())
        .map(|(idx, _)| idx)
}

fn basic_pitch_frame_to_time(frame_id: usize) -> f32 {
    let original_time =
        frame_id as f32 * BASIC_PITCH_FFT_HOP as f32 / BASIC_PITCH_SAMPLE_RATE as f32;
    let window_number = (frame_id / BASIC_PITCH_ANNOT_N_FRAMES) as f32;
    let window_offset = (BASIC_PITCH_FFT_HOP as f32 / BASIC_PITCH_SAMPLE_RATE as f32)
        * (BASIC_PITCH_ANNOT_N_FRAMES as f32
            - (BASIC_PITCH_AUDIO_N_SAMPLES as f32 / BASIC_PITCH_FFT_HOP as f32))
        + BASIC_PITCH_MAGIC_ALIGNMENT_OFFSET;

    original_time - window_offset * window_number
}

fn enframe(x: &ArrayView2<f32>, segment_samples: usize) -> Array2<f32> {
    // Ensure that the number of audio samples is divisible by segment_samples
    assert!(x.shape()[1].is_multiple_of(segment_samples));

    let mut batch: Vec<Array2<f32>> = Vec::new();
    let mut pointer = 0;

    let total_samples = x.shape()[1];

    // Enframe the sequence into smaller segments
    while pointer + segment_samples <= total_samples {
        let segment = x
            .slice(s![.., pointer..(pointer + segment_samples)])
            .to_owned();
        batch.push(segment);
        pointer += segment_samples / 2;
    }

    // Concatenate the segments along the first axis (the segment axis)
    concatenate(Axis(0), &batch.iter().map(|a| a.view()).collect::<Vec<_>>()).unwrap()
}

// TODO: Rewrite this madness
fn deframe(x: &Array3<f32>) -> Array2<f32> {
    // Get the shape of the input (N, segment_frames, classes_num)
    let (n_segments, segment_frames, _classes_num) = x.dim();

    // If there is only one segment, return it as is (removing the outer dimension)
    if n_segments == 1 {
        return x.index_axis(Axis(0), 0).to_owned(); // Equivalent to `x[0]` in Python
    }

    // Remove the last frame from each segment
    let x = x.slice(s![.., 0..segment_frames - 1, ..]).to_owned();

    // Ensure that segment_frames is divisible by 4
    let segment_samples = segment_frames - 1;
    assert!(segment_samples % 4 == 0);

    // Collect segments into a vector to concatenate them later
    let mut y: Vec<Array2<f32>> = Vec::new();

    // Append the first 75% of the first segment
    y.push(x.slice(s![0, 0..(segment_samples * 3 / 4), ..]).to_owned());

    // Append the middle part (25% to 75%) of the middle segments
    for i in 1..(n_segments - 1) {
        y.push(
            x.slice(s![i, (segment_samples / 4)..(segment_samples * 3 / 4), ..])
                .to_owned(),
        );
    }

    // Append the last 75% of the last segment
    y.push(
        x.slice(s![n_segments - 1, (segment_samples / 4).., ..])
            .to_owned(),
    );

    // Concatenate all parts along the first axis (frames axis)
    concatenate(Axis(0), &y.iter().map(|a| a.view()).collect::<Vec<_>>()).unwrap()
}

fn get_binarized_output_from_regression(
    reg_output: &ArrayView2<f32>,
    threshold: f32,
    neighbour: usize,
) -> (Array2<bool>, Array2<f32>) {
    let (frames_num, classes_num) = reg_output.dim();

    let mut binary_output = Array2::<bool>::default((frames_num, classes_num));
    let mut shift_output = Array2::<f32>::zeros((frames_num, classes_num));

    for k in 0..classes_num {
        let x: ArrayView1<f32> = reg_output.slice(ndarray::s![.., k]);

        for n in neighbour..(frames_num - neighbour) {
            if x[n] > threshold && is_monotonic_neighbour(&x, n, neighbour) {
                binary_output[[n, k]] = true;

                // See Section III-D in [1] for deduction.
                // [1] Q. Kong, et al., High-resolution Piano Transcription
                // with Pedals by Regressing Onsets and Offsets Times, 2020.
                let shift = if x[n - 1] > x[n + 1] {
                    (x[n + 1] - x[n - 1]) / (x[n] - x[n + 1]) / 2.0
                } else {
                    (x[n + 1] - x[n - 1]) / (x[n] - x[n - 1]) / 2.0
                };
                shift_output[[n, k]] = shift;
            }
        }
    }

    (binary_output, shift_output)
}

fn is_monotonic_neighbour(x: &ArrayView1<f32>, n: usize, neighbour: usize) -> bool {
    // Ensure the value of 'n' is within a valid range.
    // The caller loops over n in neighbour..(frames_num - neighbour), so this
    // branch is unreachable in practice, but we guard defensively.
    if n < neighbour || n + neighbour >= x.len() {
        return false;
    }

    for i in 0..neighbour {
        if x[n - i] < x[n - i - 1] {
            return false;
        }
        if x[n + i] < x[n + i + 1] {
            return false;
        }
    }

    true
}

fn note_detection_with_onset_offset_regress(
    frame: ArrayView2<f32>,
    onset: ArrayView2<bool>,
    onset_shift: ArrayView2<f32>,
    offset: ArrayView2<bool>,
    offset_shift: ArrayView2<f32>,
    velocity: (),
    frame_threshold: f32,
) -> midly::Smf<'static> {
    let classes_num = frame.dim().1;

    let mut notes = Vec::new();
    for piano_note in 0..classes_num {
        let res = note_detection_with_onset_offset_regress_inner(
            frame.slice(ndarray::s![.., piano_note]),
            onset.slice(ndarray::s![.., piano_note]),
            onset_shift.slice(ndarray::s![.., piano_note]),
            offset.slice(ndarray::s![.., piano_note]),
            offset_shift.slice(ndarray::s![.., piano_note]),
            velocity,
            frame_threshold,
        );

        for (bgn, fin, bgn_shift, fin_shift) in res {
            let onset_time = (bgn as f32 + bgn_shift) / FRAMES_PER_SECOND;
            let offset_time = (fin as f32 + fin_shift) / FRAMES_PER_SECOND;

            let labels: [&str; 12] = [
                "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
            ];

            let label = labels[(piano_note + 9) % labels.len()];

            // 21 is the first note in 88 keys layout
            let piano_note = piano_note + 21;

            notes.push((piano_note, onset_time, offset_time));
            log::debug!("{piano_note} {label}: {onset_time} - {offset_time}");
        }
    }

    create_midi_file(notes)
}

fn note_detection_with_onset_offset_regress_inner(
    frame: ArrayView1<f32>,
    onset: ArrayView1<bool>,
    onset_shift: ArrayView1<f32>,
    offset: ArrayView1<bool>,
    offset_shift: ArrayView1<f32>,
    _velocity: (),
    frame_threshold: f32,
) -> Vec<(usize, usize, f32, f32)> {
    let iter = frame
        .into_iter()
        .zip(onset)
        .zip(onset_shift)
        .zip(offset)
        .zip(offset_shift)
        .enumerate()
        // God forgive my sins
        .map(|(id, ((((a, b), c), d), e))| (id, a, b, c, d, e));

    let mut output_tuples = Vec::new();
    let mut bgn: Option<(usize, f32)> = None;
    let mut frame_disappear: Option<(usize, f32)> = None;
    let mut offset_occur: Option<(usize, f32)> = None;

    let len = onset.shape()[0];

    for (i, frame, onset, onset_shift, offset, offset_shift) in iter {
        if *onset {
            // Onset detected
            if let Some((bgn, bgn_offset)) = bgn {
                // Consecutive onsets. E.g., pedal is not released, but two
                // consecutive notes being played.
                let fin = i.saturating_sub(1);
                output_tuples.push((bgn, fin, bgn_offset, 0.0));

                frame_disappear = None;
                offset_occur = None;
            }

            bgn = Some((i, *onset_shift));
        }

        if let Some((bgn_time, bgn_shift)) = bgn
            && i > bgn_time
        {
            // If onset found, then search offset

            if *frame <= frame_threshold && frame_disappear.is_none() {
                // Frame disappear detected
                frame_disappear = Some((i, *offset_shift));
            }

            if *offset && offset_occur.is_none() {
                // Offset detected
                offset_occur = Some((i, *offset_shift));
            }

            if let Some((frame_disappear_time, frame_disappear_shift)) = frame_disappear {
                let (fin, fin_shift) = match offset_occur {
                    Some((offset_occur, shift))
                        if offset_occur - bgn_time > frame_disappear_time - offset_occur =>
                    {
                        // bgn --------- offset_occur --- frame_disappear
                        (offset_occur, shift)
                    }
                    _ => {
                        // bgn --- offset_occur --------- frame_disappear
                        (frame_disappear_time, frame_disappear_shift)
                    }
                };
                output_tuples.push((bgn_time, fin, bgn_shift, fin_shift));

                bgn = None;
                frame_disappear = None;
                offset_occur = None;
            }

            if let Some((bgn_time, bgn_shift)) = bgn
                && (i - bgn_time >= 600 || i == len - 1)
            {
                // Offset not detected
                let fin = i;
                output_tuples.push((bgn_time, fin, bgn_shift, *offset_shift));

                bgn = None;
                frame_disappear = None;
                offset_occur = None;
            }
        }
    }

    output_tuples.sort_by_key(|v| v.0);

    output_tuples
}

fn create_midi_file(notes: Vec<(usize, f32, f32)>) -> midly::Smf<'static> {
    let ticks_per_beat = 384;
    let beats_per_second = 2;
    let ticks_per_second = ticks_per_beat * beats_per_second;
    let microseconds_per_beat = (1_000_000.0 / beats_per_second as f64) as u32;

    let mut track1 = vec![];

    let mut message_roll = vec![];

    for (midi_note, start, end) in notes {
        message_roll.push((start, midi_note, 100));
        message_roll.push((end, midi_note, 0));
    }

    message_roll.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    let mut previous_ticks = 0;

    let start_time = 0.0;
    for message in message_roll {
        let this_ticks = ((message.0 - start_time) * ticks_per_second as f32) as i32;

        if this_ticks >= 0 {
            let diff_ticks = this_ticks - previous_ticks;
            previous_ticks = this_ticks;

            track1.push(midly::TrackEvent {
                delta: (diff_ticks as u32).into(),
                kind: midly::TrackEventKind::Midi {
                    channel: 0.into(),
                    message: midly::MidiMessage::NoteOn {
                        key: (message.1 as u8).into(),
                        vel: message.2.into(),
                    },
                },
            });
        }
    }

    track1.push(midly::TrackEvent {
        delta: 1.into(),
        kind: midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
    });

    midly::Smf {
        header: midly::Header {
            format: midly::Format::Parallel,
            timing: midly::Timing::Metrical(ticks_per_beat.into()),
        },
        tracks: vec![
            vec![
                midly::TrackEvent {
                    delta: 0.into(),
                    kind: midly::TrackEventKind::Meta(midly::MetaMessage::Tempo(
                        microseconds_per_beat.into(),
                    )),
                },
                midly::TrackEvent {
                    delta: 0.into(),
                    kind: midly::TrackEventKind::Meta(midly::MetaMessage::TimeSignature(
                        4, 2, 24, 8,
                    )),
                },
                midly::TrackEvent {
                    delta: 1.into(),
                    kind: midly::TrackEventKind::Meta(midly::MetaMessage::EndOfTrack),
                },
            ],
            track1,
        ],
    }
}
