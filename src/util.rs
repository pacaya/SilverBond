use std::path::Path;

use chrono::Utc;

pub fn safe_name(name: &str) -> anyhow::Result<String> {
    let trimmed = name.trim();
    anyhow::ensure!(!trimmed.is_empty(), "name is required");
    anyhow::ensure!(trimmed.len() <= 255, "name too long");
    anyhow::ensure!(
        !trimmed.contains("..")
            && !trimmed.contains('/')
            && !trimmed.contains('\\')
            && !trimmed.contains('%')
            && !trimmed.contains('\0'),
        "invalid path characters"
    );
    anyhow::ensure!(
        trimmed
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | ' ' | '.')),
        "invalid path characters"
    );
    Ok(trimmed.to_string())
}

pub fn now_iso() -> String {
    Utc::now().to_rfc3339()
}

pub fn slugify_filename(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for ch in name.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | ' ') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    out.trim().to_string()
}

pub fn djb2(input: &str) -> u32 {
    let mut hash: u32 = 5381;
    for byte in input.bytes() {
        hash = ((hash << 5).wrapping_add(hash)).wrapping_add(byte as u32);
    }
    hash
}

pub fn ensure_dir(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path)?;
    Ok(())
}
