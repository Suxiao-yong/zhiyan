// LLM API Key 的安全存储：使用 OS 凭据管理器（Windows Credential Manager / DPAPI）。
// OS 托管加密，非机器指纹派生、非明文，满足"apiKey 加密存储"铁律。
// master 凭据由 OS 管理，无需用户输入密码（无感）。

use keyring::Entry;

fn entry_for(provider: &str) -> Result<Entry, keyring::Error> {
    Entry::new("zhiyan", provider)
}

/// Rust-callable API key lookup reused by the LLM provider. Returns `None` on
/// `NoEntry` so callers can degrade to local mode; surfaces other keyring errors.
// ponytail: consumed by agent::llm::openai_compatible in the next task; remove
// this allow once the provider lands.
#[allow(dead_code)]
pub fn api_key_for(provider: &str) -> Result<Option<String>, keyring::Error> {
    let entry = entry_for(provider)?;
    match entry.get_password() {
        Ok(password) => Ok(Some(password)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::api_key_for;

    #[test]
    fn missing_api_key_resolves_to_none_without_panicking() {
        // A provider name unique to this process never has a stored entry.
        let provider = format!("test-missing-{}", uuid::Uuid::new_v4());
        assert_eq!(api_key_for(&provider).unwrap(), None);
    }
}

#[tauri::command]
pub fn store_api_key(provider: String, key: String) -> Result<(), String> {
    let entry = entry_for(&provider).map_err(|e| e.to_string())?;
    entry.set_password(&key).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn load_api_key(provider: String) -> Result<Option<String>, String> {
    let entry = entry_for(&provider).map_err(|e| e.to_string())?;
    match entry.get_password() {
        Ok(p) => Ok(Some(p)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn delete_api_key(provider: String) -> Result<(), String> {
    let entry = entry_for(&provider).map_err(|e| e.to_string())?;
    match entry.delete_credential() {
        Ok(()) => Ok(()),
        Err(keyring::Error::NoEntry) => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}
