use serde::{Deserialize, Serialize};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

const MODELS_URL: &str = "https://api.typesafe.ai/v1/models";
const MAX_RESPONSE_BYTES: usize = 65_536;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConnectionStatus {
    model: String,
    checked_at: u64,
}

#[derive(Deserialize)]
struct ModelsResponse {
    models: Vec<Model>,
}

#[derive(Deserialize)]
struct Model {
    name: String,
}

fn http_error(status: u16) -> String {
    match status {
        401 => "TypeSafe rejected the saved API key. Replace it and test again.",
        403 => "This key does not have permission to access TypeSafe models.",
        402 => "TypeSafe requires billing or account credit before this key can be used.",
        429 => "TypeSafe is rate limiting requests. Wait briefly and test again.",
        500..=599 => "TypeSafe is temporarily unavailable. Test again later.",
        _ => "TypeSafe returned an unexpected response. The connection was not verified.",
    }
    .into()
}

async fn probe(client: &reqwest::Client, url: &str, key: &str) -> Result<String, String> {
    let mut response = client
        .get(url)
        .bearer_auth(key)
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| {
            if error.is_timeout() {
                "TypeSafe did not respond within 15 seconds. Check your connection and retry."
            } else {
                "Could not connect securely to TypeSafe. Check your internet connection and retry."
            }
            .to_string()
        })?;
    if !response.status().is_success() {
        // Provider bodies can contain request details; never return or log them.
        return Err(http_error(response.status().as_u16()));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| "Could not read TypeSafe's response. Test again.")?
    {
        if body.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
            return Err("TypeSafe returned an oversized response. Connection not verified.".into());
        }
        body.extend_from_slice(&chunk);
    }
    let result: ModelsResponse = serde_json::from_slice(&body)
        .map_err(|_| "TypeSafe returned an invalid model list. Connection not verified.")?;
    result
        .models
        .into_iter()
        .filter(|model| {
            model.name.starts_with("jev-")
                && model.name.len() <= 128
                && model
                    .name
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"-._".contains(&byte))
        })
        .min_by_key(|model| model.name != "jev-latest")
        .map(|model| model.name)
        .ok_or_else(|| {
            "The key was accepted, but this account lists no accessible JEV models.".into()
        })
}

/// Authentication-only check documented at https://docs.typesafe.ai/models.
/// The key stays in the native process; no transcripts or inference requests are sent.
#[tauri::command]
pub async fn test_jev_connection() -> Result<ConnectionStatus, String> {
    let key = tauri::async_runtime::spawn_blocking(|| {
        super::credential_entry()?
            .get_password()
            .map_err(|error| super::credential_error("read", error))
    })
    .await
    .map_err(|_| "Could not access the saved key. Reopen the app and retry.")??;
    super::validate_api_key(&key)?;
    let client = reqwest::Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(15))
        .build()
        .map_err(|_| "Could not prepare a secure TypeSafe connection.")?;
    let model = probe(&client, MODELS_URL, &key).await?;
    Ok(ConnectionStatus {
        model,
        checked_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| "The system clock is invalid.")?
            .as_secs(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;

    fn check_response(status: u16, body: &str) -> Result<String, String> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/v1/models", listener.local_addr().unwrap());
        let body = body.to_owned();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut block = [0; 1024];
            while !request.windows(4).any(|window| window == b"\r\n\r\n") {
                let count = stream.read(&mut block).unwrap();
                assert!(count > 0);
                request.extend_from_slice(&block[..count]);
            }
            let request = String::from_utf8(request).unwrap().to_lowercase();
            assert!(request.starts_with("get /v1/models http/1.1"));
            assert!(request.contains("authorization: bearer synthetic-test-key\r\n"));
            write!(
                stream,
                "HTTP/1.1 {status} Test\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        });
        let result = tauri::async_runtime::block_on(async {
            let client = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(5))
                .build()
                .unwrap();
            probe(&client, &url, "synthetic-test-key").await
        });
        server.join().unwrap();
        result
    }

    #[test]
    fn authenticated_model_list_is_required_for_validation() {
        assert_eq!(
            check_response(
                200,
                r#"{"models":[{"name":"jev-preview"},{"name":"jev-latest"}]}"#
            )
            .unwrap(),
            "jev-latest"
        );
        assert!(check_response(200, r#"{"models":[]}"#).is_err());
        assert!(check_response(200, "not JSON").is_err());
        assert!(check_response(200, &"x".repeat(MAX_RESPONSE_BYTES + 1)).is_err());
    }

    #[test]
    fn rejected_and_transient_responses_never_validate_or_expose_bodies() {
        for status in [301, 401, 402, 403, 429, 500, 529] {
            let error = check_response(status, "synthetic-secret-in-provider-error").unwrap_err();
            assert!(!error.contains("synthetic-secret"));
            assert_eq!(error, http_error(status));
        }
    }
}
