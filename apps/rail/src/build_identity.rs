pub fn windows_version(version: &str) -> Result<[u16; 4], String> {
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 {
        return Err("Application version must be MAJOR.MINOR.PATCH".into());
    }
    let mut result = [0; 4];
    for (index, part) in parts.iter().enumerate() {
        result[index] = part
            .parse::<u16>()
            .map_err(|_| "Version components must be integers in 0..=65535".to_string())?;
    }
    Ok(result)
}

pub fn source_label(commit: Option<&str>, dirty: Option<bool>) -> String {
    match (commit, dirty) {
        (Some(commit), Some(dirty)) => format!(
            "{}{}",
            &commit[..commit.len().min(12)],
            if dirty { ", dirty" } else { "" }
        ),
        _ => "unknown source".into(),
    }
}

pub fn product_version(version: &str, commit: Option<&str>, dirty: Option<bool>) -> String {
    match (commit, dirty) {
        (Some(commit), Some(dirty)) => format!(
            "{version}+g{}{}",
            &commit[..commit.len().min(12)],
            if dirty { ".dirty" } else { "" }
        ),
        _ => format!("{version}+unknown"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_components_are_bounded() {
        assert_eq!(windows_version("0.1.65535").unwrap(), [0, 1, 65535, 0]);
        for invalid in ["0.1.65536", "0.1", "0.1.0-preview", "-1.0.0"] {
            assert!(windows_version(invalid).is_err(), "{invalid}");
        }
    }

    #[test]
    fn identity_distinguishes_clean_dirty_and_unknown() {
        let commit = Some("0123456789abcdef");
        assert_eq!(source_label(commit, Some(false)), "0123456789ab");
        assert_eq!(source_label(commit, Some(true)), "0123456789ab, dirty");
        assert_eq!(source_label(None, None), "unknown source");
        assert_eq!(
            product_version("0.1.0", commit, Some(false)),
            "0.1.0+g0123456789ab"
        );
        assert_eq!(
            product_version("0.1.0", commit, Some(true)),
            "0.1.0+g0123456789ab.dirty"
        );
        assert_eq!(product_version("0.1.0", None, None), "0.1.0+unknown");
    }
}
