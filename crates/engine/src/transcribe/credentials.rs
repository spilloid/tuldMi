//! API keys live in the OS credential store, never settings, logs, or transcript exports.
use crate::{Error, Result};

fn unavailable() -> Error {
    Error::Other("could not access the secure API key store".into())
}

pub fn validate_key(key: &str) -> Result<&str> {
    let key = key.trim();
    if key.len() < 8 || key.len() > 512 || key.chars().any(|c| c.is_control() || c.is_whitespace())
    {
        return Err(Error::Other("enter a valid API key without spaces".into()));
    }
    Ok(key)
}

#[cfg(target_os = "windows")]
fn entry() -> Result<keyring::Entry> {
    keyring::Entry::new("com.spillerstech.tuldmi", "openai-transcription")
        .map_err(|_| unavailable())
}

pub fn read() -> Result<Option<String>> {
    #[cfg(target_os = "windows")]
    {
        match entry()?.get_password() {
            Ok(key) => Ok(Some(key)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(_) => Err(unavailable()),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(Error::Other(
            "secure API key storage is supported on Windows in this release".into(),
        ))
    }
}

pub fn set(key: &str) -> Result<()> {
    let key = validate_key(key)?;
    #[cfg(target_os = "windows")]
    {
        entry()?.set_password(key).map_err(|_| unavailable())
    }
    #[cfg(not(target_os = "windows"))]
    {
        let _ = key;
        Err(unavailable())
    }
}

pub fn delete() -> Result<()> {
    #[cfg(target_os = "windows")]
    {
        match entry()?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(_) => Err(unavailable()),
        }
    }
    #[cfg(not(target_os = "windows"))]
    {
        Err(unavailable())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rejects_header_injection_and_never_echoes_keys_in_errors() {
        for input in [
            "short",
            "sk-fixture\r\nInjected: secret",
            "sk-contains spaces",
        ] {
            let message = validate_key(input).unwrap_err().to_string();
            assert!(!message.contains(input));
        }
        assert_eq!(
            validate_key("  sk-fixture-placeholder-key  ").unwrap(),
            "sk-fixture-placeholder-key"
        );
    }
}
