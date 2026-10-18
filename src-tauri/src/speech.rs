use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sherpa_onnx::{OnlineRecognizer, OnlineRecognizerConfig};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::{Arc, Mutex, OnceLock, mpsc};
use std::time::{Duration, Instant};
use tauri::{AppHandle, Emitter, Manager, State};

#[derive(Clone, Deserialize)]
struct ModelFile {
    role: String,
    name: String,
    url: String,
    size: u64,
    sha256: String,
}

#[derive(Clone, Deserialize)]
struct ModelSpec {
    id: String,
    name: String,
    description: String,
    license: String,
    source: String,
    revision: String,
    files: Vec<ModelFile>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelInfo {
    id: String,
    name: String,
    description: String,
    license: String,
    source: String,
    download_bytes: u64,
    installed: bool,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct DownloadProgress {
    model_id: String,
    downloaded: u64,
    total: u64,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SpeechEvent {
    session_id: String,
    phase: &'static str,
    text: String,
    message: Option<String>,
}

#[derive(Default)]
struct Operations {
    download: Option<Arc<AtomicBool>>,
    recording: Option<(String, Arc<AtomicU8>)>,
}

#[derive(Default, Clone)]
pub struct SpeechState(Arc<Mutex<Operations>>);

// A lease keeps download and microphone operations mutually exclusive, including
// while workers are starting or unwinding. No native inference runs on the UI thread.
struct OperationLease {
    state: SpeechState,
    download: bool,
}

impl Drop for OperationLease {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.0.lock() {
            if self.download {
                state.download = None;
            } else {
                state.recording = None;
            }
        }
    }
}

fn catalog() -> &'static [ModelSpec] {
    static MODELS: OnceLock<Vec<ModelSpec>> = OnceLock::new();
    MODELS.get_or_init(|| {
        serde_json::from_str(include_str!("../models.json")).expect("Invalid bundled model catalog")
    })
}

fn model(id: &str) -> Result<&'static ModelSpec, String> {
    catalog()
        .iter()
        .find(|m| m.id == id)
        .ok_or_else(|| "Unknown speech model. Choose a model from the list.".into())
}

fn model_root(app: &AppHandle, spec: &ModelSpec) -> Result<PathBuf, String> {
    Ok(app
        .path()
        .app_local_data_dir()
        .map_err(|_| "Could not locate local model storage.")?
        .join("models")
        .join(&spec.id))
}

fn installed(spec: &ModelSpec, root: &Path) -> bool {
    fs::read_to_string(root.join(".ready")).is_ok_and(|value| value == spec.revision)
        && spec.files.iter().all(|file| {
            fs::metadata(root.join(&file.name))
                .is_ok_and(|meta| meta.is_file() && meta.len() == file.size)
        })
}

fn verify_file(path: &Path, spec: &ModelFile) -> Result<(), String> {
    let mut file =
        File::open(path).map_err(|_| "Model file is missing. Download the model again.")?;
    if file
        .metadata()
        .map_err(|_| "Could not inspect model file.")?
        .len()
        != spec.size
    {
        return Err("Model file is incomplete. Download the model again.".into());
    }
    let mut digest = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| "Could not verify model file.")?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    if digest
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>()
        != spec.sha256
    {
        return Err("Model integrity check failed. Download the model again.".into());
    }
    Ok(())
}

#[tauri::command]
pub fn list_models(app: AppHandle) -> Result<Vec<ModelInfo>, String> {
    catalog()
        .iter()
        .map(|spec| {
            Ok(ModelInfo {
                id: spec.id.clone(),
                name: spec.name.clone(),
                description: spec.description.clone(),
                license: spec.license.clone(),
                source: spec.source.clone(),
                download_bytes: spec.files.iter().map(|f| f.size).sum(),
                installed: installed(spec, &model_root(&app, spec)?),
            })
        })
        .collect()
}

async fn download_files(
    app: &AppHandle,
    spec: &ModelSpec,
    root: &Path,
    canceled: &AtomicBool,
) -> Result<(), String> {
    fs::create_dir_all(root)
        .map_err(|_| "Could not create model storage. Check available disk space.")?;
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(1800))
        .read_timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not initialize model downloader.")?;
    let total = spec.files.iter().map(|f| f.size).sum();
    let mut downloaded = 0;
    for file in &spec.files {
        if canceled.load(Ordering::Relaxed) {
            return Err("Download canceled.".into());
        }
        let destination = root.join(&file.name);
        if verify_file(&destination, file).is_ok() {
            downloaded += file.size;
            let _ = app.emit(
                "model-download-progress",
                DownloadProgress {
                    model_id: spec.id.clone(),
                    downloaded,
                    total,
                },
            );
            continue;
        }
        let partial = destination.with_extension("part");
        let result = (async {
            let mut response = client
                .get(&file.url)
                .send()
                .await
                .and_then(|r| r.error_for_status())
                .map_err(|_| "Model download failed. Check your connection and retry.")?;
            let mut output = File::create(&partial)
                .map_err(|_| "Could not write model download. Check disk space.")?;
            let mut received = 0;
            let mut last_event = Instant::now();
            loop {
                if canceled.load(Ordering::Relaxed) {
                    return Err("Download canceled.".to_string());
                }
                let Some(buffer) = response
                    .chunk()
                    .await
                    .map_err(|_| "Download interrupted. Retry to continue setup.")?
                else {
                    break;
                };
                received += buffer.len() as u64;
                if received > file.size {
                    return Err("Model download exceeded the expected file size.".into());
                }
                output
                    .write_all(&buffer)
                    .map_err(|_| "Could not save model download. Check disk space.")?;
                if last_event.elapsed() >= Duration::from_millis(100) {
                    let _ = app.emit(
                        "model-download-progress",
                        DownloadProgress {
                            model_id: spec.id.clone(),
                            downloaded: downloaded + received,
                            total,
                        },
                    );
                    last_event = Instant::now();
                }
            }
            output
                .sync_all()
                .map_err(|_| "Could not finish saving model download.")?;
            drop(output);
            verify_file(&partial, file)?;
            if canceled.load(Ordering::Relaxed) {
                return Err("Download canceled.".into());
            }
            // Windows cannot rename over an existing file.
            if destination.exists() {
                fs::remove_file(&destination)
                    .map_err(|_| "Could not replace incomplete model file.")?;
            }
            fs::rename(&partial, &destination)
                .map_err(|_| "Could not finish installing model file.")?;
            Ok(())
        })
        .await;
        if result.is_err() {
            let _ = fs::remove_file(&partial);
        }
        result?;
        downloaded += file.size;
        let _ = app.emit(
            "model-download-progress",
            DownloadProgress {
                model_id: spec.id.clone(),
                downloaded,
                total,
            },
        );
    }
    if canceled.load(Ordering::Relaxed) {
        return Err("Download canceled.".into());
    }
    fs::write(root.join(".ready"), &spec.revision)
        .map_err(|_| "Could not finish model installation.")?;
    Ok(())
}

#[tauri::command]
pub async fn download_model(
    app: AppHandle,
    state: State<'_, SpeechState>,
    model_id: String,
) -> Result<(), String> {
    let spec = model(&model_id)?;
    let root = model_root(&app, spec)?;
    let canceled = Arc::new(AtomicBool::new(false));
    {
        let mut operations = state
            .0
            .lock()
            .map_err(|_| "Speech engine is unavailable.")?;
        if operations.download.is_some() || operations.recording.is_some() {
            return Err("Stop the current operation before downloading a model.".into());
        }
        operations.download = Some(canceled.clone());
    }
    let lease = OperationLease {
        state: state.inner().clone(),
        download: true,
    };
    let _lease = lease;
    download_files(&app, spec, &root, &canceled).await
}

#[tauri::command]
pub fn cancel_model_download(state: State<'_, SpeechState>) -> Result<(), String> {
    if let Some(cancel) = &state
        .0
        .lock()
        .map_err(|_| "Speech engine is unavailable.")?
        .download
    {
        cancel.store(true, Ordering::Relaxed);
    }
    Ok(())
}

#[tauri::command]
pub fn remove_model(
    app: AppHandle,
    state: State<'_, SpeechState>,
    model_id: String,
) -> Result<(), String> {
    let operations = state
        .0
        .lock()
        .map_err(|_| "Speech engine is unavailable.")?;
    if operations.download.is_some() || operations.recording.is_some() {
        return Err("Stop the current operation before removing a model.".into());
    }
    let root = model_root(&app, model(&model_id)?)?;
    if root.exists() {
        fs::remove_dir_all(root).map_err(|_| "Could not remove the downloaded model.")?;
    }
    Ok(())
}

fn create_recognizer(spec: &ModelSpec, root: &Path) -> Result<OnlineRecognizer, String> {
    for file in &spec.files {
        verify_file(&root.join(&file.name), file)?;
    }
    let file_path = |role: &str| -> Option<String> {
        spec.files
            .iter()
            .find(|f| f.role == role)
            .map(|f| root.join(&f.name).to_string_lossy().into_owned())
    };
    let mut config = OnlineRecognizerConfig::default();
    config.model_config.transducer.encoder = file_path("encoder");
    config.model_config.transducer.decoder = file_path("decoder");
    config.model_config.transducer.joiner = file_path("joiner");
    config.model_config.tokens = file_path("tokens");
    config.model_config.model_type = Some("zipformer2".into());
    config.model_config.num_threads = 2;
    config.enable_endpoint = false;
    OnlineRecognizer::create(&config)
        .ok_or_else(|| "Could not load the speech model. Re-download it and try again.".into())
}

fn emit_speech(
    app: &AppHandle,
    id: &str,
    phase: &'static str,
    text: &str,
    message: Option<String>,
) {
    let _ = app.emit(
        "speech-update",
        SpeechEvent {
            session_id: id.into(),
            phase,
            text: text.into(),
            message,
        },
    );
}

fn input_stream<T>(
    device: &cpal::Device,
    config: &cpal::StreamConfig,
    tx: mpsc::SyncSender<Vec<f32>>,
    failed: Arc<AtomicBool>,
) -> Result<cpal::Stream, String>
where
    T: cpal::SizedSample,
    f32: cpal::FromSample<T>,
{
    let channels = config.channels as usize;
    let callback_failed = failed.clone();
    device.build_input_stream(*config, move |data: &[T], _| {
        let mono = data.chunks(channels).map(|frame| frame.iter().map(|&sample| sample.to_sample::<f32>()).sum::<f32>() / frame.len() as f32).collect();
        if tx.try_send(mono).is_err() { callback_failed.store(true, Ordering::Relaxed); }
    }, move |_| { failed.store(true, Ordering::Relaxed); }, None)
        .map_err(|_| "Could not open the microphone. Enable microphone access for Decider in system privacy settings, then retry.".into())
}

fn run_microphone(
    app: &AppHandle,
    spec: &ModelSpec,
    root: &Path,
    id: &str,
    stop: &AtomicU8,
) -> Result<(), String> {
    let recognizer = create_recognizer(spec, root)?;
    if stop.load(Ordering::Relaxed) != 0 {
        return Ok(());
    }
    let stream = recognizer.create_stream();
    let device = cpal::default_host()
        .default_input_device()
        .ok_or("No default microphone found. Connect or select a microphone in system settings.")?;
    let supported = device
        .default_input_config()
        .map_err(|_| "Could not read microphone settings. Check system microphone permissions.")?;
    let config = supported.config();
    let sample_rate = config.sample_rate as i32;
    let (tx, rx) = mpsc::sync_channel(128);
    let failed = Arc::new(AtomicBool::new(false));
    let microphone = match supported.sample_format() {
        cpal::SampleFormat::F32 => input_stream::<f32>(&device, &config, tx, failed.clone()),
        cpal::SampleFormat::I16 => input_stream::<i16>(&device, &config, tx, failed.clone()),
        cpal::SampleFormat::U16 => input_stream::<u16>(&device, &config, tx, failed.clone()),
        cpal::SampleFormat::I32 => input_stream::<i32>(&device, &config, tx, failed.clone()),
        _ => Err("This microphone's audio format is unsupported. Choose another input device in system settings.".into()),
    }?;
    if stop.load(Ordering::Relaxed) != 0 {
        return Ok(());
    }
    microphone
        .play()
        .map_err(|_| "Could not start the microphone. Check system privacy settings.")?;
    emit_speech(app, id, "listening", "", None);
    let started = Instant::now();
    let mut previous = String::new();
    while stop.load(Ordering::Relaxed) == 0 && started.elapsed() < Duration::from_secs(60) {
        if failed.load(Ordering::Relaxed) {
            return Err("Microphone capture failed or processing fell behind. Try the compact model or check your input device.".into());
        }
        match rx.recv_timeout(Duration::from_millis(25)) {
            Ok(samples) => {
                stream.accept_waveform(sample_rate, &samples);
                while recognizer.is_ready(&stream) {
                    recognizer.decode(&stream);
                }
                if let Some(result) = recognizer.get_result(&stream)
                    && result.text != previous
                {
                    previous = result.text;
                    emit_speech(app, id, "partial", previous.trim(), None);
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(_) => return Err("The microphone disconnected. Reconnect it and retry.".into()),
        }
    }
    drop(microphone);
    if stop.load(Ordering::Relaxed) == 2 {
        return Ok(());
    }
    if failed.load(Ordering::Relaxed) {
        return Err("Microphone capture was interrupted. Please retry.".into());
    }
    emit_speech(app, id, "finalizing", &previous, None);
    for samples in rx.try_iter() {
        stream.accept_waveform(sample_rate, &samples);
        while recognizer.is_ready(&stream) {
            recognizer.decode(&stream);
        }
    }
    stream.accept_waveform(sample_rate, &vec![0.0; sample_rate as usize / 2]);
    stream.input_finished();
    while recognizer.is_ready(&stream) {
        recognizer.decode(&stream);
    }
    if stop.load(Ordering::Relaxed) == 2 {
        return Ok(());
    }
    let text = recognizer
        .get_result(&stream)
        .map(|r| r.text.trim().to_string())
        .unwrap_or_default();
    emit_speech(
        app,
        id,
        "final",
        &text,
        if text.is_empty() {
            Some("No speech recognized. Check your microphone and try again.".into())
        } else {
            None
        },
    );
    Ok(())
}

#[tauri::command]
pub fn start_transcription(
    app: AppHandle,
    state: State<'_, SpeechState>,
    model_id: String,
    session_id: String,
) -> Result<(), String> {
    if session_id.is_empty() || session_id.len() > 128 {
        return Err("Invalid recording session.".into());
    }
    let spec = model(&model_id)?;
    let root = model_root(&app, spec)?;
    if !installed(spec, &root) {
        return Err("Download this model before recording.".into());
    }
    let stop = Arc::new(AtomicU8::new(0));
    {
        let mut operations = state
            .0
            .lock()
            .map_err(|_| "Speech engine is unavailable.")?;
        if operations.download.is_some() || operations.recording.is_some() {
            return Err("Another speech operation is still running. Wait for it to finish.".into());
        }
        operations.recording = Some((session_id.clone(), stop.clone()));
    }
    let lease = OperationLease {
        state: state.inner().clone(),
        download: false,
    };
    std::thread::Builder::new()
        .name("local-transcription".into())
        .spawn(move || {
            emit_speech(&app, &session_id, "loading", "", None);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_microphone(&app, spec, &root, &session_id, &stop)
            }));
            match result {
                Ok(Err(message)) => emit_speech(&app, &session_id, "error", "", Some(message)),
                Err(_) => emit_speech(
                    &app,
                    &session_id,
                    "error",
                    "",
                    Some("Speech processing stopped unexpectedly. Please retry.".into()),
                ),
                _ => {}
            }
            drop(lease);
            emit_speech(&app, &session_id, "stopped", "", None);
        })
        .map_err(|_| "Could not start speech processing.".to_string())?;
    Ok(())
}

#[tauri::command]
pub fn stop_transcription(
    state: State<'_, SpeechState>,
    session_id: String,
    cancel: bool,
) -> Result<(), String> {
    request_stop(state.inner(), &session_id, cancel)
}

fn request_stop(state: &SpeechState, session_id: &str, cancel: bool) -> Result<(), String> {
    if let Some((active_id, stop)) = &state
        .0
        .lock()
        .map_err(|_| "Speech engine is unavailable.")?
        .recording
        && active_id == session_id
    {
        stop.fetch_max(if cancel { 2 } else { 1 }, Ordering::Relaxed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_is_bounded_and_complete() {
        assert!(model("../../arbitrary-path").is_err());
        for model in catalog() {
            assert_eq!(model.files.len(), 4);
            for file in &model.files {
                assert!(!file.name.contains('/'));
                assert!(!file.name.contains('\\'));
                assert!(file.url.starts_with("https://huggingface.co/"));
                assert!(file.url.contains(&model.revision));
                assert_eq!(file.sha256.len(), 64);
            }
        }
    }

    #[test]
    fn rejects_truncated_and_corrupted_downloads() {
        let root = std::env::temp_dir().join(format!("decider-integrity-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let path = root.join("sample");
        let spec = ModelFile {
            role: "tokens".into(),
            name: "sample".into(),
            url: String::new(),
            size: 4,
            sha256: Sha256::digest(b"test")
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect(),
        };
        fs::write(&path, b"tes").unwrap();
        assert!(verify_file(&path, &spec).is_err());
        fs::write(&path, b"best").unwrap();
        assert!(verify_file(&path, &spec).is_err());
        fs::write(&path, b"test").unwrap();
        assert!(verify_file(&path, &spec).is_ok());
        assert!(!installed(
            &ModelSpec {
                id: "test".into(),
                name: String::new(),
                description: String::new(),
                license: String::new(),
                source: String::new(),
                revision: "revision".into(),
                files: vec![spec]
            },
            &root
        ));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn stale_stop_cannot_cancel_another_session_and_cancel_wins() {
        let state = SpeechState::default();
        let stop = Arc::new(AtomicU8::new(0));
        state.0.lock().unwrap().recording = Some(("active".into(), stop.clone()));
        request_stop(&state, "stale", true).unwrap();
        assert_eq!(stop.load(Ordering::Relaxed), 0);
        request_stop(&state, "active", true).unwrap();
        request_stop(&state, "active", false).unwrap();
        assert_eq!(stop.load(Ordering::Relaxed), 2);
        let lease = OperationLease {
            state: state.clone(),
            download: false,
        };
        drop(lease);
        assert!(state.0.lock().unwrap().recording.is_none());
    }

    #[test]
    #[ignore = "Requires downloaded model and public test WAV in DECIDER_SPEECH_TEST_DIR"]
    fn transcribes_real_audio() {
        let root = PathBuf::from(std::env::var("DECIDER_SPEECH_TEST_DIR").unwrap());
        let recognizer = create_recognizer(model("zipformer-en-int8").unwrap(), &root).unwrap();
        let wave = sherpa_onnx::Wave::read(root.join("sample.wav").to_str().unwrap()).unwrap();
        let stream = recognizer.create_stream();
        for samples in wave.samples().chunks(1600) {
            stream.accept_waveform(wave.sample_rate(), samples);
            while recognizer.is_ready(&stream) {
                recognizer.decode(&stream);
            }
        }
        stream.accept_waveform(
            wave.sample_rate(),
            &vec![0.0; wave.sample_rate() as usize / 2],
        );
        stream.input_finished();
        while recognizer.is_ready(&stream) {
            recognizer.decode(&stream);
        }
        let text = recognizer.get_result(&stream).unwrap().text.to_lowercase();
        assert!(
            text.contains("after early nightfall"),
            "Unexpected transcript: {text}"
        );
    }
}
