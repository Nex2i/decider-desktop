mod commands;
mod desktop;
mod jev;
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
    Entry::new(SERVICE, ACCOUNT).map_err(|error| credential_error("access", error))
}

fn credential_error(action: &str, error: KeyringError) -> String {
    // Never format variants that can contain credential bytes or entry details.
    let reason = match error {
        KeyringError::PlatformFailure(error) | KeyringError::NoStorageAccess(error) => {
            format!(
                "{error}. Unlock your credential store and allow Decider access if prompted. If the app was rebuilt while open, reopen it and retry."
            )
        }
        KeyringError::NoEntry => "No saved key was found.".into(),
        KeyringError::BadEncoding(_) | KeyringError::BadDataFormat(_, _) => {
            "The stored key could not be decoded. Replace it with a valid API key.".into()
        }
        _ => "The OS credential store is unavailable. Reopen the app and retry.".into(),
    };
    format!("Could not {action} the JEV key. {reason}")
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
async fn app_status() -> Result<AppStatus, String> {
    tauri::async_runtime::spawn_blocking(read_app_status)
        .await
        .map_err(|_| "Could not read credential status. Please retry.".to_string())?
}

fn read_app_status() -> Result<AppStatus, String> {
    let configured = match credential_entry()?.get_password() {
        Ok(key) => !key.is_empty(),
        Err(KeyringError::NoEntry) => false,
        Err(error) => return Err(credential_error("read", error)),
    };
    Ok(AppStatus {
        platform: std::env::consts::OS,
        version: env!("CARGO_PKG_VERSION"),
        jev_key_configured: configured,
    })
}

#[tauri::command]
async fn save_jev_key(api_key: String) -> Result<AppStatus, String> {
    tauri::async_runtime::spawn_blocking(move || save_key(api_key))
        .await
        .map_err(|_| "Could not save the key. Please retry.".to_string())?
}

fn save_key(api_key: String) -> Result<AppStatus, String> {
    let key = validate_api_key(&api_key)?;
    credential_entry()?
        .set_password(key)
        .map_err(|error| credential_error("save", error))?;
    let saved = credential_entry()?
        .get_password()
        .map_err(|error| credential_error("verify", error))?;
    if saved != key {
        return Err("The credential store did not retain the key. Please retry.".into());
    }
    Ok(AppStatus {
        platform: std::env::consts::OS,
        version: env!("CARGO_PKG_VERSION"),
        jev_key_configured: true,
    })
}

#[tauri::command]
async fn remove_jev_key() -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(remove_key)
        .await
        .map_err(|_| "Could not remove the key. Please retry.".to_string())?
}

fn remove_key() -> Result<(), String> {
    match credential_entry()?.delete_credential() {
        Ok(()) | Err(KeyringError::NoEntry) => Ok(()),
        Err(error) => Err(credential_error("remove", error)),
    }
}

pub fn run() {
    tauri::Builder::default()
        .manage(speech::SpeechState::default())
        .manage(desktop::DesktopState::default())
        .manage(commands::CommandState::default())
        .invoke_handler(tauri::generate_handler![
            app_status,
            commands::start_desktop_command,
            commands::resolve_desktop_command,
            save_jev_key,
            remove_jev_key,
            jev::test_jev_connection,
            desktop::inspect_desktop,
            desktop::request_desktop_access,
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
    use super::{credential_error, validate_api_key};

    #[test]
    fn credential_errors_do_not_expose_secret_bytes() {
        let error = keyring::Error::BadEncoding(b"secret-test-value".to_vec());
        let message = credential_error("read", error);
        assert!(message.contains("could not be decoded"));
        assert!(!message.contains("secret-test-value"));
        let error =
            keyring::Error::PlatformFailure(Box::new(std::io::Error::other("OS access denied")));
        assert!(credential_error("save", error).contains("OS access denied"));
    }

    #[test]
    #[ignore = "Uses an isolated test credential in the real OS credential store"]
    fn credential_survives_reopening_entry() {
        let service = format!("ai.nex2i.decider.test.{}", std::process::id());
        let entry = keyring::Entry::new(&service, "test-only").unwrap();
        entry.set_password("synthetic-test-key").unwrap();
        let reopened = keyring::Entry::new(&service, "test-only").unwrap();
        let matches = reopened.get_password().unwrap() == "synthetic-test-key";
        entry.delete_credential().unwrap();
        assert!(matches, "A new entry must retrieve the saved credential");
        assert!(matches!(
            reopened.get_password(),
            Err(keyring::Error::NoEntry)
        ));
    }

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
