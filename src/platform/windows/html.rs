use anyhow::{Result, ensure};

pub fn decode(bytes: &[u8]) -> Result<String> {
    let header = String::from_utf8_lossy(&bytes[..bytes.len().min(4096)]);
    let offset = |name: &str| -> Option<usize> {
        header
            .lines()
            .find_map(|line| line.strip_prefix(name)?.trim().parse().ok())
    };
    let start = offset("StartFragment:").ok_or_else(|| anyhow::anyhow!("missing HTML fragment"))?;
    let end = offset("EndFragment:").ok_or_else(|| anyhow::anyhow!("missing HTML fragment"))?;
    ensure!(start <= end && end <= bytes.len(), "invalid HTML fragment");
    Ok(std::str::from_utf8(&bytes[start..end])?.into())
}
pub fn encode(fragment: &str) -> Vec<u8> {
    let header = |a, b, c, d| {
        format!(
            "Version:1.0\r\nStartHTML:{a:010}\r\nEndHTML:{b:010}\r\nStartFragment:{c:010}\r\nEndFragment:{d:010}\r\n"
        )
    };
    let start = header(0, 0, 0, 0).len();
    let prefix = "<html><body><!--StartFragment-->";
    let suffix = "<!--EndFragment--></body></html>";
    let first = start + prefix.len();
    let last = first + fragment.len();
    format!(
        "{}{prefix}{fragment}{suffix}\0",
        header(start, last + suffix.len(), first, last)
    )
    .into_bytes()
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fragment_offsets_are_utf8_bytes_and_bounded() {
        let fragment = "<b>中文 🦀</b>";
        assert_eq!(decode(&encode(fragment)).unwrap(), fragment);
        assert!(decode(b"StartFragment:9999\r\nEndFragment:10000").is_err());
    }
}
