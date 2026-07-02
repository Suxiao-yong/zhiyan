// LLM API Key 的安全存储：使用 OS 凭据管理器（Windows Credential Manager / DPAPI）。
// OS 托管加密，非机器指纹派生、非明文，满足"apiKey 加密存储"铁律。
// master 凭据由 OS 管理，无需用户输入密码（无感）。

use keyring::Entry;

fn entry_for(provider: &str) -> Result<Entry, keyring::Error> {
    Entry::new("zhiyan", provider)
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
