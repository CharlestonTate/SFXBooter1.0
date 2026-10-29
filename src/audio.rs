use std::num::{NonZeroU16, NonZeroU32};
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, Sample, SampleFormat, SizedSample};
use flacenc::component::BitRepr;
use flacenc::error::Verify;
use hound::{SampleFormat as WavFormat, WavReader, WavSpec, WavWriter};
use rodio::buffer::SamplesBuffer;
use rodio::{DeviceSinkBuilder, MixerDeviceSink, Player, Source};

pub struct Recorded {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

pub struct Recorder {
    samples: Arc<Mutex<Vec<f32>>>,
    peak: Arc<AtomicU32>,
    channels: u16,
    sample_rate: u32,
    started: Instant,
    stream: cpal::Stream,
}

#[derive(Clone)]
pub struct LoadedWav {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl LoadedWav {
    pub fn frames(&self) -> usize {
        if self.channels == 0 {
            0
        } else {
            self.samples.len() / self.channels as usize
        }
    }

    pub fn duration(&self) -> f32 {
        if self.sample_rate == 0 {
            0.0
        } else {
            self.frames() as f32 / self.sample_rate as f32
        }
    }
}

#[derive(Clone, Copy)]
pub enum RegionEdit {
    Trim,
    Cut,
}

pub struct Peaks {
    pub min: Vec<f32>,
    pub max: Vec<f32>,
}

pub struct Playback {
    player: Player,
    // The device handle must stay alive for as long as `player` is playing.
    _sink: MixerDeviceSink,
}

// one row in the microphone menu. id is the cpal "host:device" string.
#[derive(Clone)]
pub struct InputChoice {
    pub id: String,
    pub name: String,
}

// what a new recording is written as. the extension is part of the choice
// so two formats never share the same file ending.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum AudioFormat {
    Wav,
    Aiff,
    Flac,
}

impl AudioFormat {
    pub const ALL: [AudioFormat; 3] = [AudioFormat::Wav, AudioFormat::Aiff, AudioFormat::Flac];

    pub fn label(self) -> &'static str {
        match self {
            AudioFormat::Wav => "WAV",
            AudioFormat::Aiff => "AIFF",
            AudioFormat::Flac => "FLAC",
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            AudioFormat::Wav => "wav",
            AudioFormat::Aiff => "aiff",
            AudioFormat::Flac => "flac",
        }
    }

    pub fn as_str(self) -> &'static str {
        self.extension()
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "wav" => Some(AudioFormat::Wav),
            "aiff" | "aif" => Some(AudioFormat::Aiff),
            "flac" => Some(AudioFormat::Flac),
            _ => None,
        }
    }

    pub fn from_path(path: &Path) -> Option<Self> {
        path.extension()
            .and_then(|ext| ext.to_str())
            .and_then(Self::parse)
    }
}

// friendly name, falling back when the driver hands us a blank one
fn device_label(device: &cpal::Device) -> String {
    let name = device
        .description()
        .ok()
        .map(|description| description.name().trim().to_string())
        .unwrap_or_default();
    if name.is_empty() {
        "Input".to_string()
    } else {
        name
    }
}

// called when the menu opens, not every frame. a device that cannot record is skipped.
pub fn list_inputs() -> Result<Vec<InputChoice>, String> {
    let host = cpal::default_host();
    let devices = host.input_devices().map_err(|err| err.to_string())?;
    let mut choices = Vec::new();
    for device in devices {
        if device.default_input_config().is_err() {
            continue;
        }
        let id = device.id().map_err(|err| err.to_string())?;
        choices.push(InputChoice {
            id: id.to_string(),
            name: device_label(&device),
        });
    }
    choices.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then_with(|| a.id.cmp(&b.id))
    });
    if choices.is_empty() {
        return Err("No input device.".into());
    }
    Ok(choices)
}

pub fn default_input() -> Result<InputChoice, String> {
    let host = cpal::default_host();
    let device = host
        .default_input_device()
        .ok_or_else(|| "No input device.".to_string())?;
    device
        .default_input_config()
        .map_err(|err| err.to_string())?;
    let id = device.id().map_err(|err| err.to_string())?;
    Ok(InputChoice {
        id: id.to_string(),
        name: device_label(&device),
    })
}

impl Recorder {
    // none uses the default input. some is a saved "host:device" id.
    pub fn start(device_id: Option<&str>) -> Result<Self, String> {
        let host = cpal::default_host();
        let device = match device_id {
            Some(id) if !id.is_empty() => {
                let parsed = cpal::DeviceId::from_str(id)
                    .map_err(|err| format!("Saved microphone could not be read: {err}"))?;
                // the id was stored the last time this mic was pluged in
                host.device_by_id(&parsed)
                    .ok_or_else(|| "That microphone is not connected anymore.".to_string())?
            }
            _ => host
                .default_input_device()
                .ok_or_else(|| "No input device.".to_string())?,
        };
        let supported = device
            .default_input_config()
            .map_err(|err| err.to_string())?;
        let channels = supported.channels();
        let sample_rate = supported.sample_rate();
        if channels == 0 || sample_rate == 0 {
            return Err("Input device reported an unusable format.".into());
        }

        let samples = Arc::new(Mutex::new(Vec::<f32>::with_capacity(
            sample_rate as usize * channels as usize,
        )));
        let peak = Arc::new(AtomicU32::new(0));
        let format = supported.sample_format();
        let config = supported.config();
        let stream = match format {
            SampleFormat::F32 => build_stream::<f32>(&device, config, &samples, &peak)?,
            SampleFormat::F64 => build_stream::<f64>(&device, config, &samples, &peak)?,
            SampleFormat::I8 => build_stream::<i8>(&device, config, &samples, &peak)?,
            SampleFormat::I16 => build_stream::<i16>(&device, config, &samples, &peak)?,
            SampleFormat::I32 => build_stream::<i32>(&device, config, &samples, &peak)?,
            SampleFormat::U8 => build_stream::<u8>(&device, config, &samples, &peak)?,
            SampleFormat::U16 => build_stream::<u16>(&device, config, &samples, &peak)?,
            SampleFormat::U32 => build_stream::<u32>(&device, config, &samples, &peak)?,
            other => return Err(format!("Unsupported input format: {other}")),
        };
        stream.play().map_err(|err| err.to_string())?;

        Ok(Self {
            samples,
            peak,
            channels,
            sample_rate,
            started: Instant::now(),
            stream,
        })
    }

    pub fn elapsed(&self) -> Duration {
        self.started.elapsed()
    }

    pub fn level(&self) -> f32 {
        let level = f32::from_bits(self.peak.load(Ordering::Relaxed));
        if level.is_finite() {
            level.clamp(0.0, 1.0)
        } else {
            0.0
        }
    }

    pub fn finish(self) -> Recorded {
        drop(self.stream);
        let mut samples = self
            .samples
            .lock()
            .map(|mut guard| std::mem::take(&mut *guard))
            .unwrap_or_default();
        let channels = self.channels.max(1) as usize;
        let extra = samples.len() % channels;
        if extra != 0 {
            samples.truncate(samples.len() - extra);
        }
        Recorded {
            sample_rate: self.sample_rate,
            channels: self.channels,
            samples,
        }
    }
}

fn build_stream<T>(
    device: &cpal::Device,
    config: cpal::StreamConfig,
    samples: &Arc<Mutex<Vec<f32>>>,
    peak: &Arc<AtomicU32>,
) -> Result<cpal::Stream, String>
where
    T: SizedSample + Copy,
    f32: FromSample<T>,
{
    let samples = Arc::clone(samples);
    let peak = Arc::clone(peak);
    device
        .build_input_stream(
            config,
            move |data: &[T], _| push_samples(data, &samples, &peak),
            |err| eprintln!("input stream error: {err}"),
            None,
        )
        .map_err(|err| err.to_string())
}

fn push_samples<T>(data: &[T], samples: &Mutex<Vec<f32>>, peak: &AtomicU32)
where
    T: Copy,
    f32: FromSample<T>,
{
    let mut max = 0.0f32;
    let Ok(mut buffer) = samples.lock() else {
        return;
    };
    buffer.reserve(data.len());
    for sample in data {
        let value = f32::from_sample(*sample);
        max = max.max(value.abs());
        buffer.push(value);
    }
    peak.store(max.to_bits(), Ordering::Relaxed);
}

pub fn write_recorded(path: &Path, recorded: &Recorded, format: AudioFormat) -> Result<(), String> {
    let samples = recorded
        .samples
        .iter()
        .copied()
        .map(f32_to_i16)
        .collect::<Vec<_>>();
    write_clip(
        path,
        recorded.sample_rate,
        recorded.channels,
        &samples,
        format,
    )
}

// writes the clip in whatever format the path (or the picker) asked for.
// edits call this too, so a flac stays a flac.
pub fn write_clip(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    samples: &[i16],
    format: AudioFormat,
) -> Result<(), String> {
    if sample_rate == 0 || channels == 0 || samples.is_empty() {
        return Err("Cannot write an empty clip.".into());
    }
    if samples.len() % channels as usize != 0 {
        return Err("Sample data is not aligned to frames.".into());
    }
    match format {
        AudioFormat::Wav => write_i16_wav(path, sample_rate, channels, samples),
        AudioFormat::Aiff => write_aiff(path, sample_rate, channels, samples),
        AudioFormat::Flac => write_flac(path, sample_rate, channels, samples),
    }
}

pub fn write_i16_wav(
    path: &Path,
    sample_rate: u32,
    channels: u16,
    samples: &[i16],
) -> Result<(), String> {
    let spec = wav_spec(sample_rate, channels)?;
    write_samples(path, spec, samples.iter().copied())
}

pub fn read_wav(path: &Path) -> Result<LoadedWav, String> {
    let mut reader = WavReader::open(path).map_err(|err| err.to_string())?;
    let spec = reader.spec();
    if spec.channels == 0 || spec.sample_rate == 0 {
        return Err("WAV file has an unusable format.".into());
    }
    let mut samples = match (spec.sample_format, spec.bits_per_sample) {
        (WavFormat::Float, 32) => reader
            .samples::<f32>()
            .map(|sample| sample.map(f32_to_i16).map_err(|err| err.to_string()))
            .collect::<Result<Vec<_>, _>>()?,
        (WavFormat::Int, bits) if (8..=32).contains(&bits) => reader
            .samples::<i32>()
            .map(|sample| {
                sample
                    .map(|value| int_to_i16(value, bits))
                    .map_err(|err| err.to_string())
            })
            .collect::<Result<Vec<_>, _>>()?,
        (format, bits) => {
            return Err(format!("Unsupported WAV format ({format:?}, {bits} bits)."));
        }
    };
    let channels = spec.channels as usize;
    let extra = samples.len() % channels;
    if extra != 0 {
        samples.truncate(samples.len() - extra);
    }
    Ok(LoadedWav {
        sample_rate: spec.sample_rate,
        channels: spec.channels,
        samples,
    })
}

pub fn read_clip(path: &Path) -> Result<LoadedWav, String> {
    match AudioFormat::from_path(path) {
        Some(AudioFormat::Wav) => read_wav(path),
        Some(AudioFormat::Aiff) => read_aiff(path),
        Some(AudioFormat::Flac) => read_flac(path),
        None => Err("That file is not a WAV, AIFF, or FLAC clip.".into()),
    }
}

pub fn clip_duration(path: &Path) -> Result<f32, String> {
    match AudioFormat::from_path(path) {
        Some(AudioFormat::Wav) => wav_duration(path),
        Some(AudioFormat::Aiff) => aiff_duration(path),
        Some(AudioFormat::Flac) => flac_duration_file(path),
        None => Err("Unknown clip format.".into()),
    }
}

pub fn wav_duration(path: &Path) -> Result<f32, String> {
    let reader = WavReader::open(path).map_err(|err| err.to_string())?;
    let spec = reader.spec();
    if spec.sample_rate == 0 {
        return Ok(0.0);
    }
    Ok(reader.duration() as f32 / spec.sample_rate as f32)
}

pub fn edit_samples(
    samples: &[i16],
    channels: u16,
    start: usize,
    end: usize,
    edit: RegionEdit,
) -> Result<Vec<i16>, String> {
    if channels == 0 {
        return Err("Clip has no channels.".into());
    }
    let width = channels as usize;
    if samples.len() % width != 0 {
        return Err("Sample data is not aligned to frames.".into());
    }
    let frames = samples.len() / width;
    if start >= end || end > frames {
        return Err("Selection is empty.".into());
    }
    let from = start * width;
    let to = end * width;
    let edited = match edit {
        RegionEdit::Trim => samples[from..to].to_vec(),
        RegionEdit::Cut => {
            if from == 0 && to == samples.len() {
                return Err("That would delete the whole clip.".into());
            }
            let mut kept = Vec::with_capacity(samples.len() - (to - from));
            kept.extend_from_slice(&samples[..from]);
            kept.extend_from_slice(&samples[to..]);
            kept
        }
    };
    if edited.len() < width {
        return Err("Clip would be empty.".into());
    }
    Ok(edited)
}

pub fn build_peaks(samples: &[i16], channels: u16, buckets: usize) -> Peaks {
    let channels = channels.max(1) as usize;
    let frames = samples.len() / channels;
    if frames == 0 {
        return Peaks {
            min: vec![0.0],
            max: vec![0.0],
        };
    }
    let buckets = buckets.clamp(1, frames);
    let mut min = vec![0.0f32; buckets];
    let mut max = vec![0.0f32; buckets];
    for frame in 0..frames {
        let base = frame * channels;
        let mut mixed = 0.0f32;
        for channel in 0..channels {
            mixed += samples[base + channel] as f32;
        }
        let value = mixed / channels as f32 / 32768.0;
        let bucket = (frame * buckets / frames).min(buckets - 1);
        min[bucket] = min[bucket].min(value);
        max[bucket] = max[bucket].max(value);
    }
    Peaks { min, max }
}

impl Playback {
    pub fn open() -> Result<Self, String> {
        let mut sink = DeviceSinkBuilder::open_default_sink().map_err(|err| err.to_string())?;
        sink.log_on_drop(false);
        let player = Player::connect_new(sink.mixer());
        Ok(Self {
            player,
            _sink: sink,
        })
    }

    pub fn play(&self, wav: &LoadedWav, start: usize, end: usize) -> Result<(), String> {
        let channels = NonZeroU16::new(wav.channels).ok_or("Clip has no channels.")?;
        let sample_rate = NonZeroU32::new(wav.sample_rate).ok_or("Clip has no sample rate.")?;
        let width = wav.channels as usize;
        let from = start * width;
        let to = end * width;
        if from >= to || to > wav.samples.len() {
            return Err("Nothing to play.".into());
        }
        let data = wav.samples[from..to]
            .iter()
            .map(|sample| *sample as f32 / 32768.0)
            .collect::<Vec<_>>();
        self.player.stop();
        self.player
            .append(SamplesBuffer::new(channels, sample_rate, data));
        self.player.play();
        Ok(())
    }

    pub fn stop(&self) {
        self.player.stop();
    }
}

fn wav_spec(sample_rate: u32, channels: u16) -> Result<WavSpec, String> {
    if sample_rate == 0 || channels == 0 {
        return Err("Cannot write a WAV with no samples.".into());
    }
    Ok(WavSpec {
        channels,
        sample_rate,
        bits_per_sample: 16,
        sample_format: WavFormat::Int,
    })
}

fn write_samples(
    path: &Path,
    spec: WavSpec,
    samples: impl IntoIterator<Item = i16>,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let temporary = temporary_path(path);
    let write_result = (|| {
        let mut writer = WavWriter::create(&temporary, spec).map_err(|err| err.to_string())?;
        for sample in samples {
            writer.write_sample(sample).map_err(|err| err.to_string())?;
        }
        writer.finalize().map_err(|err| err.to_string())
    })();
    if let Err(err) = write_result {
        let _ = std::fs::remove_file(&temporary);
        return Err(err);
    }
    replace_file(&temporary, path)
}

fn temporary_path(path: &Path) -> PathBuf {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("clip.wav");
    path.with_file_name(format!(".{name}.part"))
}

fn replace_file(temporary: &Path, destination: &Path) -> Result<(), String> {
    if !destination.exists() {
        return std::fs::rename(temporary, destination).map_err(|err| err.to_string());
    }
    let backup = temporary_path(destination).with_extension("bak");
    std::fs::rename(destination, &backup).map_err(|err| err.to_string())?;
    if let Err(err) = std::fs::rename(temporary, destination) {
        let _ = std::fs::rename(&backup, destination);
        return Err(err.to_string());
    }
    let _ = std::fs::remove_file(backup);
    Ok(())
}

// aiff is not something rodio will decode, so this is our own reader and writer.
// only the plain aiff form, 16 bit, which is all this app writes.
fn write_aiff(path: &Path, sample_rate: u32, channels: u16, samples: &[i16]) -> Result<(), String> {
    let frames = samples.len() / channels as usize;
    let mut pcm = Vec::with_capacity(samples.len() * 2);
    for sample in samples {
        pcm.extend_from_slice(&sample.to_be_bytes());
    }
    let data_pad = pcm.len() % 2;
    let comm_size = 18u32;
    let ssnd_size = (8 + pcm.len() + data_pad) as u32;
    // form size counts everything after the size field itself
    let form_size = 4 + 8 + comm_size + 8 + ssnd_size;
    let mut out = Vec::with_capacity(12 + form_size as usize);
    out.extend_from_slice(b"FORM");
    out.extend_from_slice(&form_size.to_be_bytes());
    out.extend_from_slice(b"AIFF");
    out.extend_from_slice(b"COMM");
    out.extend_from_slice(&comm_size.to_be_bytes());
    out.extend_from_slice(&(channels as i16).to_be_bytes());
    out.extend_from_slice(&(frames as u32).to_be_bytes());
    out.extend_from_slice(&16i16.to_be_bytes());
    out.extend_from_slice(&encode_extended(sample_rate));
    out.extend_from_slice(b"SSND");
    out.extend_from_slice(&ssnd_size.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&pcm);
    if data_pad == 1 {
        out.push(0);
    }
    write_bytes(path, &out)
}

fn read_aiff(path: &Path) -> Result<LoadedWav, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let (channels, frames, bits, sample_rate, pcm) = parse_aiff(&bytes)?;
    if bits != 16 {
        return Err("Only 16-bit AIFF clips can be opened.".into());
    }
    let mut samples = pcm;
    let width = channels as usize;
    if frames > 0 {
        let want = frames.saturating_mul(width);
        if samples.len() > want {
            samples.truncate(want);
        }
    }
    let extra = samples.len() % width;
    if extra != 0 {
        samples.truncate(samples.len() - extra);
    }
    Ok(LoadedWav {
        sample_rate,
        channels,
        samples,
    })
}

fn aiff_duration(path: &Path) -> Result<f32, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    let (channels, frames, _bits, sample_rate, _) = parse_aiff(&bytes)?;
    if sample_rate == 0 || channels == 0 {
        return Ok(0.0);
    }
    Ok(frames as f32 / sample_rate as f32)
}

fn parse_aiff(bytes: &[u8]) -> Result<(u16, usize, u16, u32, Vec<i16>), String> {
    if bytes.len() < 12 || &bytes[0..4] != b"FORM" || &bytes[8..12] != b"AIFF" {
        return Err("Not an AIFF file.".into());
    }
    let mut offset = 12usize;
    let mut channels = 0u16;
    let mut frames = 0usize;
    let mut bits = 0u16;
    let mut sample_rate = 0u32;
    let mut pcm = None;
    while offset + 8 <= bytes.len() {
        let id = &bytes[offset..offset + 4];
        let size = u32::from_be_bytes(bytes[offset + 4..offset + 8].try_into().unwrap()) as usize;
        let start = offset + 8;
        if start + size > bytes.len() {
            break;
        }
        if id == b"COMM" && size >= 18 {
            channels = u16::from_be_bytes([bytes[start], bytes[start + 1]]);
            frames = u32::from_be_bytes(bytes[start + 2..start + 6].try_into().unwrap()) as usize;
            bits = u16::from_be_bytes([bytes[start + 6], bytes[start + 7]]);
            sample_rate = decode_extended(&bytes[start + 8..start + 18]);
        } else if id == b"SSND" && size >= 8 {
            let audio = &bytes[start + 8..start + size];
            let mut samples = Vec::with_capacity(audio.len() / 2);
            let mut index = 0;
            while index + 1 < audio.len() {
                samples.push(i16::from_be_bytes([audio[index], audio[index + 1]]));
                index += 2;
            }
            pcm = Some(samples);
        }
        // chunks are padded out to an even lenght
        let padded = size + (size % 2);
        offset = start + padded;
    }
    if channels == 0 || sample_rate == 0 {
        return Err("AIFF file has an unusable format.".into());
    }
    let samples = pcm.ok_or("AIFF file has no audio data.")?;
    Ok((channels, frames, bits, sample_rate, samples))
}

// 80 bit extended float, just enough of it to store a whole sample rate.
// the top bit of the mantissa is the 2^(exponent - 16383) place.
fn encode_extended(rate: u32) -> [u8; 10] {
    if rate == 0 {
        return [0; 10];
    }
    let zeros = (rate as u64).leading_zeros();
    let mantissa = (rate as u64) << zeros;
    let exponent = (16383 + (63 - zeros as i32)) as u16;
    let mut out = [0u8; 10];
    out[0] = (exponent >> 8) as u8;
    out[1] = (exponent & 0xff) as u8;
    out[2..].copy_from_slice(&mantissa.to_be_bytes());
    out
}

fn decode_extended(bytes: &[u8]) -> u32 {
    if bytes.len() < 10 {
        return 0;
    }
    let exponent = u16::from_be_bytes([bytes[0], bytes[1]]) as i32;
    let mantissa = u64::from_be_bytes(bytes[2..10].try_into().unwrap());
    if mantissa == 0 {
        return 0;
    }
    let shift = exponent - 16383;
    if shift < 0 {
        return 0;
    }
    if shift >= 63 {
        let up = (shift - 63) as u32;
        if up >= 31 {
            return u32::MAX;
        }
        ((mantissa as u128) << up).min(u32::MAX as u128) as u32
    } else {
        (mantissa >> (63 - shift) as u32) as u32
    }
}

fn write_flac(path: &Path, sample_rate: u32, channels: u16, samples: &[i16]) -> Result<(), String> {
    let pcm = samples
        .iter()
        .map(|sample| *sample as i32)
        .collect::<Vec<_>>();
    let mut encoder = flacenc::config::Encoder::default();
    // one thread is plenty for a sound effect and keeps stop instant
    encoder.multithread = false;
    let config = encoder
        .into_verified()
        .map_err(|(_, err)| format!("flac setup failed: {err:?}"))?;
    let block = config
        .block_size
        .min(pcm.len() / channels.max(1) as usize)
        .max(16);
    let source =
        flacenc::source::MemSource::from_samples(&pcm, channels as usize, 16, sample_rate as usize);
    let stream = flacenc::encode_with_fixed_block_size(&config, source, block)
        .map_err(|err| format!("flac encode failed: {err:?}"))?;
    stream
        .verify()
        .map_err(|err| format!("flac verify failed: {err:?}"))?;
    let mut sink = flacenc::bitsink::ByteSink::with_capacity(stream.count_bits());
    stream
        .write(&mut sink)
        .map_err(|err| format!("flac write failed: {err:?}"))?;
    write_bytes(path, sink.as_slice())
}

fn read_flac(path: &Path) -> Result<LoadedWav, String> {
    let bytes = std::fs::read(path).map_err(|err| err.to_string())?;
    if bytes.len() < 4 || &bytes[0..4] != b"fLaC" {
        return Err("Not a FLAC file.".into());
    }
    let len = bytes.len() as u64;
    let decoder = rodio::Decoder::builder()
        .with_data(std::io::Cursor::new(bytes))
        .with_byte_len(len)
        .with_seekable(true)
        .with_hint("flac")
        .build()
        .map_err(|err| err.to_string())?;
    let channels = decoder.channels().get();
    let sample_rate = decoder.sample_rate().get();
    let mut samples = decoder.map(f32_to_i16).collect::<Vec<_>>();
    let width = channels as usize;
    let extra = samples.len() % width.max(1);
    if width > 0 && extra != 0 {
        samples.truncate(samples.len() - extra);
    }
    if channels == 0 || sample_rate == 0 {
        return Err("FLAC file has an unusable format.".into());
    }
    Ok(LoadedWav {
        sample_rate,
        channels,
        samples,
    })
}

// streaminfo sits at a fixed spot when it is the first metadata block,
// which is how every normal flac file is laid out.
fn flac_duration_file(path: &Path) -> Result<f32, String> {
    let mut file = std::fs::File::open(path).map_err(|err| err.to_string())?;
    let mut header = [0u8; 42];
    use std::io::Read;
    let read = file.read(&mut header).map_err(|err| err.to_string())?;
    if let Some(secs) = flac_header_duration(&header[..read]) {
        if secs > 0.0 {
            return Ok(secs);
        }
    }
    // some files leave the sample count at zero, so count them the slow way
    Ok(read_flac(path)?.duration())
}

fn flac_header_duration(bytes: &[u8]) -> Option<f32> {
    if bytes.len() < 42 || &bytes[0..4] != b"fLaC" {
        return None;
    }
    if bytes[4] & 0x7f != 0 {
        return None;
    }
    // 8 byte field at file offset 18: rate in the top 20 bits, frames in the low 36
    let packed = u64::from_be_bytes(bytes[18..26].try_into().ok()?);
    let rate = (packed >> 44) as u32;
    let frames = packed & ((1u64 << 36) - 1);
    if rate == 0 {
        return None;
    }
    Some(frames as f32 / rate as f32)
}

fn write_bytes(path: &Path, bytes: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    let temporary = temporary_path(path);
    if let Err(err) = std::fs::write(&temporary, bytes) {
        let _ = std::fs::remove_file(&temporary);
        return Err(err.to_string());
    }
    replace_file(&temporary, path)
}

fn f32_to_i16(sample: f32) -> i16 {
    let sample = if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    };
    (sample * 32767.0).round() as i16
}

fn int_to_i16(sample: i32, bits: u16) -> i16 {
    if bits <= 16 {
        (sample << (16 - bits)) as i16
    } else {
        (sample >> (bits - 16)) as i16
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_keeps_the_selected_frames() {
        let samples = [1, 2, 3, 4, 5, 6];
        let edited = edit_samples(&samples, 1, 1, 4, RegionEdit::Trim).unwrap();
        assert_eq!(edited, vec![2, 3, 4]);
    }

    #[test]
    fn cut_drops_the_selected_frames_and_keeps_channels_aligned() {
        let samples = [1, 2, 3, 4, 5, 6];
        let edited = edit_samples(&samples, 2, 1, 2, RegionEdit::Cut).unwrap();
        assert_eq!(edited, vec![1, 2, 5, 6]);
    }

    #[test]
    fn cut_rejects_deleting_the_whole_clip() {
        let samples = [1, 2, 3, 4];
        let error = edit_samples(&samples, 1, 0, 4, RegionEdit::Cut).unwrap_err();
        assert!(error.contains("whole clip"));
    }

    #[test]
    fn wav_roundtrip_preserves_frames() {
        let path = std::env::temp_dir().join(format!("sfxrecord-test-{}.wav", std::process::id()));
        let recorded = Recorded {
            sample_rate: 8_000,
            channels: 2,
            samples: vec![0.0, 0.5, -0.5, 1.0],
        };
        write_recorded(&path, &recorded, AudioFormat::Wav).unwrap();
        write_recorded(&path, &recorded, AudioFormat::Wav).unwrap();
        let loaded = read_wav(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.sample_rate, 8_000);
        assert_eq!(loaded.channels, 2);
        assert_eq!(loaded.frames(), 2);
        assert_eq!(loaded.samples[1], f32_to_i16(0.5));
        assert_eq!(loaded.samples[2], f32_to_i16(-0.5));
    }

    #[test]
    fn extended_rate_roundtrip_keeps_common_rates() {
        for rate in [8_000, 22_050, 44_100, 48_000, 96_000] {
            assert_eq!(decode_extended(&encode_extended(rate)), rate);
        }
    }

    #[test]
    fn aiff_roundtrip_preserves_frames() {
        let path = std::env::temp_dir().join(format!("sfxrecord-test-{}.aiff", std::process::id()));
        let samples = [0, 16_000, -16_000, 100];
        write_clip(&path, 44_100, 2, &samples, AudioFormat::Aiff).unwrap();
        let loaded = read_clip(&path).unwrap();
        let duration = clip_duration(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.samples, samples);
        assert_eq!(loaded.sample_rate, 44_100);
        assert_eq!(loaded.channels, 2);
        assert!((duration - 1.0 / 44_100.0).abs() < 0.0001);
    }

    #[test]
    fn flac_roundtrip_preserves_frames() {
        let path = std::env::temp_dir().join(format!("sfxrecord-test-{}.flac", std::process::id()));
        let samples = (0..256)
            .map(|index| (index * 40) as i16)
            .collect::<Vec<_>>();
        write_clip(&path, 8_000, 1, &samples, AudioFormat::Flac).unwrap();
        let loaded = read_clip(&path).unwrap();
        let duration = clip_duration(&path).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(loaded.sample_rate, 8_000);
        assert_eq!(loaded.channels, 1);
        assert_eq!(loaded.frames(), samples.len());
        assert!((duration - samples.len() as f32 / 8_000.0).abs() < 0.001);
        for (got, want) in loaded.samples.iter().zip(samples.iter()) {
            assert!((*got - *want).abs() <= 2);
        }
    }

    #[test]
    fn peaks_follow_the_loudest_sample_in_each_bucket() {
        let samples = [0, 16_384, -16_384, 0];
        let peaks = build_peaks(&samples, 1, 2);
        assert_eq!(peaks.max.len(), 2);
        assert!(peaks.max[0] > 0.4);
        assert!(peaks.min[1] < -0.4);
    }
}
