//! Explicitly configured server transcription; never log tokens, audio, or server bodies.
use super::{Transcript, TranscriptionProvider};
use crate::{
    Error, Result,
    storage::NewSegment,
    types::{RemoteAuthentication, RemoteTranscription},
};
use parking_lot::Mutex;
use reqwest::{
    Url,
    blocking::{Client, multipart},
    redirect::Policy,
};
use serde::{Deserialize, Serialize};
use std::{
    sync::Arc,
    time::{Duration, Instant},
};

fn error(message: &str) -> Error {
    Error::Transcription(message.into())
}
fn client(timeout: u64) -> Result<Client> {
    Client::builder()
        .redirect(Policy::none())
        .connect_timeout(Duration::from_secs(10))
        .timeout(Duration::from_secs(timeout))
        .build()
        .map_err(|_| error("could not initialize server connection"))
}
pub fn validate(config: &RemoteTranscription) -> Result<()> {
    let url = Url::parse(&config.endpoint).map_err(|_| error("invalid transcription endpoint"))?;
    let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
    if (url.scheme() != "https" && !(local && url.scheme() == "http"))
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
        || config.model.trim().is_empty()
        || config.model.len() > 128
    {
        return Err(error(
            "use an HTTPS transcription URL without credentials and a model name",
        ));
    }
    if let Some(label) = &config.library_device
        && (config.authentication != RemoteAuthentication::Authentik
            || label.trim().is_empty()
            || label.chars().count() > 64)
    {
        return Err(error(
            "choose an Authentik server and a laptop name of up to 64 characters",
        ));
    }
    if config.authentication == RemoteAuthentication::ApiKey {
        if url.as_str() != "https://api.openai.com/v1/audio/transcriptions"
            || !matches!(
                config.model.as_str(),
                "gpt-transcribe" | "gpt-4o-mini-transcribe" | "gpt-4o-transcribe" | "whisper-1"
            )
        {
            return Err(error(
                "choose a supported OpenAI transcription model at the official OpenAI endpoint",
            ));
        }
        return Ok(());
    }
    let issuer = Url::parse(&config.issuer).map_err(|_| error("invalid Authentik issuer"))?;
    if issuer.scheme() != "https"
        || issuer.host_str().is_none()
        || !issuer.username().is_empty()
        || issuer.password().is_some()
        || issuer.query().is_some()
        || issuer.fragment().is_some()
        || config.client_id.trim().is_empty()
    {
        return Err(error(
            "use an HTTPS Authentik issuer and public OAuth client ID",
        ));
    }
    Ok(())
}

#[derive(Deserialize)]
struct Discovery {
    issuer: String,
    device_authorization_endpoint: String,
    token_endpoint: String,
}
#[derive(Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}
fn default_interval() -> u64 {
    5
}
#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLogin {
    pub user_code: String,
    pub verification_uri: String,
    pub interval: u64,
    pub expires_in: u64,
}
struct Device {
    code: String,
    expires: Instant,
    next_poll: Instant,
    interval: Duration,
}
#[derive(Deserialize)]
struct Token {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: u64,
}
struct Credentials {
    token: Token,
    expires: Instant,
}
#[derive(Default)]
pub struct RemoteAuth {
    config: Option<(String, String)>,
    token_endpoint: String,
    device: Option<Device>,
    credentials: Option<Credentials>,
    revision: u64,
}
impl RemoteAuth {
    pub fn credentials_changed(&mut self) {
        self.revision = self.revision.wrapping_add(1);
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    fn matches(&self, c: &RemoteTranscription) -> bool {
        self.config
            .as_ref()
            .is_some_and(|(issuer, id)| issuer == &c.issuer && id == &c.client_id)
    }
    pub fn begin(&mut self, config: &RemoteTranscription) -> Result<DeviceLogin> {
        validate(config)?;
        if config.authentication != RemoteAuthentication::Authentik {
            return Err(error(
                "OpenAI uses the saved API key, not Authentik sign-in",
            ));
        }
        let http = client(15)?;
        let discovery: Discovery = http
            .get(format!(
                "{}/.well-known/openid-configuration",
                config.issuer.trim_end_matches('/')
            ))
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json())
            .map_err(|_| {
                error("could not discover Authentik; check issuer and device-flow configuration")
            })?;
        if discovery.issuer.trim_end_matches('/') != config.issuer.trim_end_matches('/') {
            return Err(error("Authentik issuer mismatch"));
        }
        let issuer = Url::parse(&config.issuer).map_err(|_| error("invalid issuer"))?;
        for endpoint in [
            &discovery.device_authorization_endpoint,
            &discovery.token_endpoint,
        ] {
            let url = Url::parse(endpoint).map_err(|_| error("invalid Authentik endpoint"))?;
            if url.origin() != issuer.origin() {
                return Err(error("Authentik endpoint origin mismatch"));
            }
        }
        let device: DeviceResponse = http
            .post(&discovery.device_authorization_endpoint)
            .form(&[
                ("client_id", config.client_id.as_str()),
                ("scope", "openid offline_access"),
            ])
            .send()
            .and_then(|r| r.error_for_status())
            .and_then(|r| r.json())
            .map_err(|_| error("could not start Authentik device sign-in"))?;
        let verification =
            Url::parse(&device.verification_uri).map_err(|_| error("invalid sign-in URL"))?;
        if verification.origin() != issuer.origin() {
            return Err(error("sign-in URL origin mismatch"));
        }
        self.config = Some((config.issuer.clone(), config.client_id.clone()));
        self.token_endpoint = discovery.token_endpoint;
        self.credentials = None;
        let interval = device.interval.clamp(5, 60);
        self.device = Some(Device {
            code: device.device_code,
            expires: Instant::now() + Duration::from_secs(device.expires_in.min(3600)),
            next_poll: Instant::now(),
            interval: Duration::from_secs(interval),
        });
        Ok(DeviceLogin {
            user_code: device.user_code,
            verification_uri: device.verification_uri,
            interval,
            expires_in: device.expires_in,
        })
    }
    pub fn poll(&mut self, config: &RemoteTranscription) -> Result<bool> {
        if !self.matches(config) {
            return Err(error("start sign-in again after changing server settings"));
        }
        if self.credentials.is_some() {
            return Ok(true);
        }
        let device = self
            .device
            .as_mut()
            .ok_or_else(|| error("start Authentik sign-in first"))?;
        if Instant::now() >= device.expires {
            self.device = None;
            return Err(error("sign-in expired; try again"));
        }
        if Instant::now() < device.next_poll {
            return Ok(false);
        }
        device.next_poll = Instant::now() + device.interval;
        let response = client(15)?
            .post(&self.token_endpoint)
            .form(&[
                ("client_id", config.client_id.as_str()),
                ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ("device_code", device.code.as_str()),
            ])
            .send()
            .map_err(|_| error("Authentik is unreachable"))?;
        if !response.status().is_success() {
            #[derive(Deserialize)]
            struct OAuthError {
                error: String,
            }
            let reason = response
                .json::<OAuthError>()
                .map_err(|_| error("invalid Authentik response"))?;
            return match reason.error.as_str() {
                "authorization_pending" => Ok(false),
                "slow_down" => {
                    device.interval += Duration::from_secs(5);
                    device.next_poll = Instant::now() + device.interval;
                    Ok(false)
                }
                _ => {
                    self.device = None;
                    Err(error("Authentik sign-in was denied or expired"))
                }
            };
        }
        let token: Token = response
            .json()
            .map_err(|_| error("invalid Authentik token"))?;
        self.save(token);
        self.device = None;
        Ok(true)
    }
    fn save(&mut self, token: Token) {
        self.revision = self.revision.wrapping_add(1);
        let expires =
            Instant::now() + Duration::from_secs(token.expires_in.saturating_sub(30).min(86_400));
        self.credentials = Some(Credentials { token, expires });
    }
    fn bearer(&mut self, config: &RemoteTranscription) -> Result<String> {
        if config.authentication == RemoteAuthentication::ApiKey {
            return super::credentials::read()?
                .ok_or_else(|| error("add your OpenAI API key in Transcription settings"));
        }
        if !self.matches(config) {
            return Err(error("sign in with Authentik in Transcription settings"));
        }
        let creds = self
            .credentials
            .as_ref()
            .ok_or_else(|| error("sign in with Authentik in Transcription settings"))?;
        if Instant::now() < creds.expires {
            return Ok(creds.token.access_token.clone());
        }
        let refresh = creds
            .token
            .refresh_token
            .clone()
            .ok_or_else(|| error("Authentik session expired; sign in again"))?;
        let response = client(15)?
            .post(&self.token_endpoint)
            .form(&[
                ("grant_type", "refresh_token"),
                ("refresh_token", refresh.as_str()),
                ("client_id", config.client_id.as_str()),
            ])
            .send()
            .and_then(|r| r.error_for_status())
            .map_err(|_| error("could not refresh Authentik sign-in"))?;
        let mut token: Token = response
            .json()
            .map_err(|_| error("invalid Authentik token"))?;
        if token.refresh_token.is_none() {
            token.refresh_token = Some(refresh);
        }
        self.save(token);
        Ok(self
            .credentials
            .as_ref()
            .unwrap()
            .token
            .access_token
            .clone())
    }
}

pub struct RemoteProvider {
    config: RemoteTranscription,
    http: Client,
    auth: Arc<Mutex<RemoteAuth>>,
}
impl RemoteProvider {
    pub fn new(config: RemoteTranscription, auth: Arc<Mutex<RemoteAuth>>) -> Result<Self> {
        validate(&config)?;
        Ok(Self {
            config,
            http: client(120)?,
            auth,
        })
    }

    fn transcribe_with_token(
        &self,
        samples: &[f32],
        language: Option<&str>,
        token: &str,
        job: Option<&crate::storage::Job>,
    ) -> Result<Transcript> {
        let part = multipart::Part::bytes(wav(samples))
            .file_name("speech.wav")
            .mime_str("audio/wav")
            .map_err(|_| error("could not prepare audio upload"))?;
        let mut form = multipart::Form::new()
            .part("file", part)
            .text("model", self.config.model.clone());
        let current_openai = self.config.authentication == RemoteAuthentication::ApiKey
            && self.config.model == "gpt-transcribe";
        if !current_openai {
            let format = if self.config.authentication == RemoteAuthentication::Authentik
                || self.config.model == "whisper-1"
            {
                "verbose_json"
            } else {
                "json"
            };
            form = form.text("response_format", format);
        }
        if let Some(language) = language.filter(|l| *l != "auto") {
            form = form.text(
                if current_openai {
                    "languages[]"
                } else {
                    "language"
                },
                language.to_owned(),
            );
        }
        let mut endpoint = Url::parse(&self.config.endpoint)
            .map_err(|_| error("invalid transcription endpoint"))?;
        let mut receipt = None;
        if let Some(device) = &self.config.library_device {
            let job = job.ok_or_else(|| error("shared recording requires a capture ID"))?;
            let clip = serde_json::json!({"id":job.event_id,"started":job.started_at,"device":device,"source":job.source});
            form = form.text("clip", clip.to_string());
            endpoint.set_path("/v1/library/transcriptions");
            receipt = Some(job.event_id);
        }
        let response = self
            .http
            .post(endpoint)
            .bearer_auth(token)
            .multipart(form)
            .send()
            .map_err(|_| {
                error("transcription service unreachable or timed out; audio remains queued")
            })?;
        if !response.status().is_success() {
            return Err(error(match response.status().as_u16() {
                401 | 403 if self.config.authentication == RemoteAuthentication::ApiKey => {
                    "OpenAI access denied; check the saved API key and project permissions"
                }
                401 | 403 => "server access denied; sign in with Authentik and check access policy",
                429 => {
                    "transcription rate limit or quota reached; check provider billing and limits; audio remains queued"
                }
                503 => "transcription service busy; audio remains queued",
                _ => {
                    "transcription service rejected the request; check provider/model configuration"
                }
            }));
        }
        use std::io::Read;
        let mut bytes = Vec::new();
        response
            .take(2_000_001)
            .read_to_end(&mut bytes)
            .map_err(|_| error("could not read transcription response"))?;
        if bytes.len() > 2_000_000 {
            return Err(error("transcription response too large"));
        }
        if let Some(expected) = receipt {
            #[derive(Deserialize)]
            struct Receipt {
                archived: uuid::Uuid,
            }
            if serde_json::from_slice::<Receipt>(&bytes)
                .ok()
                .map(|r| r.archived)
                != Some(expected)
            {
                return Err(error(
                    "shared library did not confirm storage; audio remains queued",
                ));
            }
        }
        parse_transcript(&bytes, samples.len() as i64 / 16)
    }
}
impl TranscriptionProvider for RemoteProvider {
    fn backend_label(&self) -> Option<&str> {
        Some(
            if self.config.authentication == RemoteAuthentication::ApiKey {
                "OpenAI"
            } else {
                "Server"
            },
        )
    }
    fn model_id(&self) -> &str {
        &self.config.model
    }
    fn transcribe(&mut self, samples: &[f32], language: Option<&str>) -> Result<Transcript> {
        self.run(samples, language, None)
    }
    fn transcribe_job(
        &mut self,
        job: &crate::storage::Job,
        samples: &[f32],
        language: Option<&str>,
    ) -> Result<Transcript> {
        self.run(samples, language, Some(job))
    }
}
impl RemoteProvider {
    fn run(
        &self,
        samples: &[f32],
        language: Option<&str>,
        job: Option<&crate::storage::Job>,
    ) -> Result<Transcript> {
        if samples.iter().any(|s| !s.is_finite()) {
            return Err(error("non-finite audio"));
        }
        if super::below_energy_gate(samples) {
            return Ok(Transcript {
                segments: vec![],
                pieces: vec![],
                language: language.filter(|l| *l != "auto").map(str::to_owned),
            });
        }
        let token = self.auth.lock().bearer(&self.config)?;
        self.transcribe_with_token(samples, language, &token, job)
    }
}

#[derive(Serialize, Deserialize)]
pub struct LibraryClip {
    id: uuid::Uuid,
    started: i64,
    device: String,
    source: crate::types::Source,
    transcript: String,
}

#[derive(Serialize, Deserialize)]
pub struct LibraryPage {
    clips: Vec<LibraryClip>,
    more: bool,
}

impl RemoteProvider {
    fn library_request(
        &self,
        method: reqwest::Method,
        path: &str,
    ) -> Result<reqwest::blocking::RequestBuilder> {
        if self.config.authentication != RemoteAuthentication::Authentik {
            return Err(error(
                "select your Authentik server in Transcription settings",
            ));
        }
        let token = self.auth.lock().bearer(&self.config)?;
        let mut url = Url::parse(&self.config.endpoint).map_err(|_| error("invalid server URL"))?;
        url.set_path(path);
        Ok(self.http.request(method, url).bearer_auth(token))
    }

    pub fn list_clips(
        &self,
        query: &str,
        before: Option<i64>,
        before_id: Option<uuid::Uuid>,
    ) -> Result<LibraryPage> {
        let mut req = self
            .library_request(reqwest::Method::GET, "/v1/library/clips")?
            .query(&[("q", query)]);
        if let (Some(time), Some(id)) = (before, before_id) {
            req = req.query(&[("before", time.to_string()), ("before_id", id.to_string())]);
        }
        let body = library_bytes(req, 4 * 1024 * 1024)?;
        serde_json::from_slice(&body).map_err(|_| error("invalid shared timeline response"))
    }

    pub fn clip_audio(&self, id: uuid::Uuid) -> Result<Vec<u8>> {
        library_bytes(
            self.library_request(
                reqwest::Method::GET,
                &format!("/v1/library/clips/{id}/audio"),
            )?,
            10 * 1024 * 1024,
        )
    }

    pub fn delete_clip(&self, id: uuid::Uuid) -> Result<()> {
        library_bytes(
            self.library_request(reqwest::Method::DELETE, &format!("/v1/library/clips/{id}"))?,
            1024,
        )?;
        Ok(())
    }
}

fn library_bytes(request: reqwest::blocking::RequestBuilder, limit: u64) -> Result<Vec<u8>> {
    use std::io::Read;
    let response = request
        .send()
        .map_err(|_| error("shared library unreachable"))?;
    if !response.status().is_success() {
        return Err(error(match response.status().as_u16() {
            401 | 403 => "sign in with Authentik in Transcription settings",
            404 => "shared library or recording unavailable",
            _ => "shared library request failed",
        }));
    }
    let mut body = Vec::new();
    response
        .take(limit + 1)
        .read_to_end(&mut body)
        .map_err(|_| error("incomplete shared library response"))?;
    if body.len() as u64 > limit {
        return Err(error("shared library response too large"));
    }
    Ok(body)
}

fn parse_transcript(bytes: &[u8], duration_ms: i64) -> Result<Transcript> {
    let body: Response =
        serde_json::from_slice(bytes).map_err(|_| error("invalid transcription response"))?;
    let mut segments = Vec::new();
    for s in body.segments {
        if !s.start.is_finite() || !s.end.is_finite() || s.start < 0.0 || s.end < s.start {
            return Err(error("invalid server timestamps"));
        }
        if !s.text.trim().is_empty() {
            segments.push(NewSegment {
                start_ms: ((s.start * 1000.0) as i64).min(duration_ms),
                end_ms: ((s.end * 1000.0) as i64).min(duration_ms),
                text: s.text.trim().into(),
                confidence: None,
                speaker_id: None,
            });
        }
    }
    if segments.is_empty() && !body.text.trim().is_empty() {
        segments.push(NewSegment {
            start_ms: 0,
            end_ms: duration_ms,
            text: body.text.trim().into(),
            confidence: None,
            speaker_id: None,
        });
    }
    Ok(Transcript {
        segments,
        pieces: vec![],
        language: body.language.or_else(|| {
            if body.languages.len() == 1 {
                Some(body.languages[0].code.clone())
            } else {
                None
            }
        }),
    })
}
#[derive(Deserialize)]
struct Response {
    text: String,
    language: Option<String>,
    #[serde(default)]
    languages: Vec<DetectedLanguage>,
    #[serde(default)]
    segments: Vec<Segment>,
}
#[derive(Deserialize)]
struct DetectedLanguage {
    code: String,
}
#[derive(Deserialize)]
struct Segment {
    start: f64,
    end: f64,
    text: String,
}
fn wav(samples: &[f32]) -> Vec<u8> {
    let size = (samples.len() * 2) as u32;
    let mut data = Vec::with_capacity(44 + size as usize);
    data.extend(b"RIFF");
    data.extend((size + 36).to_le_bytes());
    data.extend(b"WAVEfmt ");
    data.extend(16u32.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(1u16.to_le_bytes());
    data.extend(16_000u32.to_le_bytes());
    data.extend(32_000u32.to_le_bytes());
    data.extend(2u16.to_le_bytes());
    data.extend(16u16.to_le_bytes());
    data.extend(b"data");
    data.extend(size.to_le_bytes());
    for sample in samples {
        data.extend(((sample.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
    }
    data
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn current_openai_response_keeps_clip_timing_and_reports_detected_language() {
        let transcript = parse_transcript(br#"{"text":"Fixture words","languages":[{"code":"en"}],"usage":{"type":"duration","seconds":3.5}}"#, 3500).unwrap();
        assert_eq!(transcript.language.as_deref(), Some("en"));
        assert_eq!(transcript.segments[0].start_ms, 0);
        assert_eq!(transcript.segments[0].end_ms, 3500);
        assert_eq!(transcript.segments[0].confidence, None);
        let multilingual = parse_transcript(
            br#"{"text":"Fixture words","languages":[{"code":"en"},{"code":"fr"}]}"#,
            1000,
        )
        .unwrap();
        assert_eq!(multilingual.language, None);
    }

    #[test]
    fn api_keys_are_pinned_to_the_official_openai_endpoint() {
        let mut config = RemoteTranscription {
            endpoint: "https://api.openai.com/v1/audio/transcriptions".into(),
            model: "gpt-transcribe".into(),
            issuer: String::new(),
            client_id: String::new(),
            library_device: None,
            authentication: RemoteAuthentication::ApiKey,
        };
        assert!(validate(&config).is_ok());
        config.endpoint = "https://other.example/v1/audio/transcriptions".into();
        assert!(validate(&config).is_err());
        config.endpoint = "https://api.openai.com/v1/audio/transcriptions".into();
        config.model = "claude-sonnet-5-5".into();
        assert!(validate(&config).is_err());
    }

    #[test]
    fn current_openai_upload_uses_language_hints_and_json_without_authentik() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&chunk[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .map(|s| s.trim().parse().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body = String::from_utf8_lossy(&request);
            assert!(body.contains("Bearer fixture-api-key"));
            assert!(body.contains("name=\"languages[]\""));
            assert!(!body.contains("name=\"language\""));
            assert!(!body.contains("name=\"response_format\""));
            assert!(body.contains("gpt-transcribe"));
            let json = r#"{"text":"Fixture words","languages":[{"code":"en"}]}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", json.len(), json).unwrap();
        });
        // Only this private test construction bypasses endpoint validation for loopback.
        let provider = RemoteProvider {
            config: RemoteTranscription {
                endpoint: format!("http://{address}/v1/audio/transcriptions"),
                model: "gpt-transcribe".into(),
                issuer: String::new(),
                client_id: String::new(),
                library_device: None,
                authentication: RemoteAuthentication::ApiKey,
            },
            http: client(15).unwrap(),
            auth: Arc::new(Mutex::new(RemoteAuth::default())),
        };
        let result = provider
            .transcribe_with_token(&vec![0.1; 1600], Some("en"), "fixture-api-key", None)
            .unwrap();
        assert_eq!(result.language.as_deref(), Some("en"));
        assert_eq!(result.segments[0].end_ms, 100);
        server.join().unwrap();
    }
    #[test]
    fn posts_authenticated_wav_and_reads_segment_timestamps() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
        };
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut request = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = stream.read(&mut chunk).unwrap();
                assert!(n > 0);
                request.extend_from_slice(&chunk[..n]);
                if let Some(end) = request.windows(4).position(|w| w == b"\r\n\r\n") {
                    let headers = String::from_utf8_lossy(&request[..end]).to_lowercase();
                    let length: usize = headers
                        .lines()
                        .find_map(|line| {
                            line.strip_prefix("content-length:")
                                .map(|v| v.trim().parse().unwrap())
                        })
                        .unwrap();
                    if request.len() >= end + 4 + length {
                        break;
                    }
                }
            }
            let body = String::from_utf8_lossy(&request);
            assert!(body.contains("Bearer fixture-token"));
            assert!(body.contains("speech.wav"));
            assert!(request.windows(4).any(|w| w == b"RIFF"));
            let json = r#"{"text":"Fixture words","language":"en","segments":[{"start":0.01,"end":0.09,"text":"Fixture words"}]}"#;
            write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", json.len(), json).unwrap();
        });
        let config = RemoteTranscription {
            endpoint: format!("http://{address}/v1/audio/transcriptions"),
            model: "base.en".into(),
            issuer: "https://auth.example/application/o/tuldmi/".into(),
            client_id: "desktop".into(),
            library_device: None,
            authentication: RemoteAuthentication::Authentik,
        };
        let mut auth = RemoteAuth {
            config: Some((config.issuer.clone(), config.client_id.clone())),
            ..Default::default()
        };
        auth.save(Token {
            access_token: "fixture-token".into(),
            refresh_token: None,
            expires_in: 600,
        });
        let mut provider = RemoteProvider::new(config, Arc::new(Mutex::new(auth))).unwrap();
        let transcript = provider.transcribe(&vec![0.1; 1600], Some("en")).unwrap();
        assert_eq!(transcript.segments[0].start_ms, 10);
        assert_eq!(transcript.segments[0].end_ms, 90);
        assert_eq!(transcript.segments[0].text, "Fixture words");
        server.join().unwrap();
    }
    #[test]
    fn wav_has_correct_pcm_header_and_samples() {
        let bytes = wav(&[-1.0, 0.0, 1.0]);
        assert_eq!(&bytes[..4], b"RIFF");
        assert_eq!(bytes.len(), 50);
        assert_eq!(&bytes[44..], &[1, 128, 0, 0, 255, 127]);
    }
    #[test]
    fn rejects_insecure_or_credential_bearing_endpoints() {
        let mut c = RemoteTranscription {
            endpoint: "https://tuldmi.spillerstech.us/v1/audio/transcriptions".into(),
            model: "base.en".into(),
            issuer: "https://auth.spillerstech.us/application/o/tuldmi/".into(),
            client_id: "tuldmi-desktop".into(),
            library_device: None,
            authentication: RemoteAuthentication::Authentik,
        };
        assert!(validate(&c).is_ok());
        for bad in [
            "http://remote/v1/audio/transcriptions",
            "https://user:secret@remote/",
            "https://remote/?token=secret",
        ] {
            c.endpoint = bad.into();
            assert!(validate(&c).is_err());
        }
    }
}
