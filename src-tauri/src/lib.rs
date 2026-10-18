mod qwen_tokenizer;
mod speech;

use keyring::{Entry, Error as KeyringError};
use serde::Serialize;

const SERVICE: &str = "ai.nex2i.decider";
const ACCOUNT: &str = "jev-api-key";

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppStatus {
    platform: &'static str,
    version: &'static str,
    jev_key_configured: bool,
}

fn credential_entry() -> Result<Entry, String> {
    Entry::new(SERVICE, ACCOUNT).map_err(|_| "Could not access the OS credential store.".into())
}

fn validate_api_key(key: &str) -> Result<&str, String> {
    let key = key.trim();
    if key.is_empty() || key.len() > 4096 || key.chars().any(char::is_control) {
        return Err(
            "Enter a nonempty API key without control characters (maximum 4096 bytes).".into(),
        );
    }
    Ok(key)
}

#[tauri::command]
fn app_status() -> Result<AppStatus, String> {
    let configured = match credential_entry()?.get_password() {
        Ok(key) => !key.is_empty(),
        Err(KeyringError::NoEntry) => false,
        Err(_) => {
            return Err(
                "Could not read the OS credential store. Try again after unlocking it.".into(),
            );
        }
    };
    Ok(AppStatus {
        platform: std::env::consts::OS,
        version: env!("CARGO_PKG_VERSION"),
        jev_key_configured: configured,
    })
}

#[tauri::command]
fn save_jev_key(api_key: String) -> Result<(), String> {
    let key = validate_api_key(&api_key)?;
    credential_entry()?
        .set_password(key)
        .map_err(|_| "Could not save the key in the OS credential store.".into())
}

#[tauri::command]
fn remove_jev_key() -> Result<(), String> {
    match credential_entry()?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(_) => Err("Could not remove the key from the OS credential store.".into()),
    }
}

pub fn run() {
    tauri::Builder::default()
        .manage(speech::SpeechState::default())
        .invoke_handler(tauri::generate_handler![
            app_status,
            save_jev_key,
            remove_jev_key,
            speech::list_models,
            speech::download_model,
            speech::cancel_model_download,
            speech::remove_model,
            speech::start_transcription,
            speech::stop_transcription
        ])
        .run(tauri::generate_context!())
        .expect("error while running Decider Desktop");
}

#[cfg(test)]
mod tests {
    use super::validate_api_key;

    #[test]
    fn rejects_empty_oversized_and_control_character_keys() {
        for key in [
            "".to_owned(),
            "   ".to_owned(),
            "a\nb".to_owned(),
            "x".repeat(4097),
        ] {
            assert!(validate_api_key(&key).is_err());
        }
    }

    #[test]
    fn trims_surrounding_whitespace_without_changing_key() {
        assert_eq!(validate_api_key("  example-key  ").unwrap(), "example-key");
    }
}
