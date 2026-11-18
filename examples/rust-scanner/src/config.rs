use std::path::Path;

/// The configuration the scanner reads from one directory.
pub struct ScannerConfig {
    pub root: String,
}

/// Refuses a directory that holds no `config.json`.
pub fn parse_config(dir: &Path) -> Result<ScannerConfig, String> {
    let path = dir.join("config.json");
    if !path.is_file() {
        return Err(format!("{} holds no config.json", dir.display()));
    }
    Ok(ScannerConfig {
        root: dir.display().to_string(),
    })
}

fn normalise(name: &str) -> String {
    name.trim().to_string()
}
