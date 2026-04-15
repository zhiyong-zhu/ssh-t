use anyhow::Result;

const SERVICE_NAME: &str = "ssh-t";

/// Credential store backed by OS keyring (macOS Keychain, Linux Secret Service, Windows Credential Manager).
pub struct CredentialStore;

impl CredentialStore {
    /// Store a password for a host in the OS keyring.
    pub fn store_password(host: &str, user: &str, password: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, &format!("{user}@{host}"))?;
        entry.set_password(password)?;
        Ok(())
    }

    /// Retrieve a stored password from the OS keyring.
    pub fn get_password(host: &str, user: &str) -> Result<String> {
        let entry = keyring::Entry::new(SERVICE_NAME, &format!("{user}@{host}"))?;
        let password = entry.get_password()?;
        Ok(password)
    }

    /// Delete a stored password from the OS keyring.
    pub fn delete_password(host: &str, user: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, &format!("{user}@{host}"))?;
        entry.delete_credential()?;
        Ok(())
    }

    /// Store a key passphrase in the OS keyring.
    pub fn store_key_passphrase(key_path: &str, passphrase: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE_NAME, &format!("key:{key_path}"))?;
        entry.set_password(passphrase)?;
        Ok(())
    }

    /// Retrieve a stored key passphrase from the OS keyring.
    pub fn get_key_passphrase(key_path: &str) -> Result<String> {
        let entry = keyring::Entry::new(SERVICE_NAME, &format!("key:{key_path}"))?;
        let passphrase = entry.get_password()?;
        Ok(passphrase)
    }
}
