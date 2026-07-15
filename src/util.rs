use std::path::{Path, PathBuf};

pub(crate) fn stable_hash(parts: &[&str]) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in parts {
        hasher.update(&(part.len() as u64).to_le_bytes());
        hasher.update(part.as_bytes());
    }
    hasher.finalize().to_hex().to_string()
}

pub(crate) fn new_run_dir(cwd: &Path) -> PathBuf {
    let now = jiff::Zoned::now();
    let timestamp = now.strftime("%Y%m%d-%H%M%S").to_string();
    let id = stable_hash(&[&now.timestamp().as_nanosecond().to_string()]);
    cwd.join("bisectrunk-runs")
        .join(format!("{timestamp}-{}", &id[..7]))
}

pub(crate) fn short_sha(sha: &str) -> &str {
    sha.get(..sha.len().min(12)).unwrap_or(sha)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn run_directory_uses_timestamp_and_short_id() {
        let path = new_run_dir(Path::new("/tmp"));
        let name = path
            .file_name()
            .and_then(|value| value.to_str())
            .expect("UTF-8 run directory name");
        assert!(
            regex::Regex::new(r"^\d{8}-\d{6}-[0-9a-f]{7}$")
                .expect("static regex")
                .is_match(name),
            "unexpected run directory name: {name}"
        );
    }
}
