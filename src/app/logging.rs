use anyhow::Result;
use flexi_logger::{
    Cleanup, Criterion, DeferredNow, FileSpec, Logger, LoggerHandle, Naming, WriteMode,
};
use std::{io::Write, path::Path, sync::OnceLock};

static LOGGER: OnceLock<LoggerHandle> = OnceLock::new();
const FILE_SIZE: u64 = 2 * 1024 * 1024;

pub(crate) fn init(dir: &Path) -> Result<()> {
    let level: log::LevelFilter = std::env::var("RUSTBOARD_LOG")
        .unwrap_or_else(|_| "info".into())
        .parse()?;
    let handle = logger(dir, level)?.start()?;
    let _ = LOGGER.set(handle);
    std::panic::set_hook(Box::new(|info| {
        if let Some(location) = info.location() {
            log::error!(target: "agent", "panic at {}:{}", location.file(), location.line());
        } else {
            log::error!(target: "agent", "panic");
        }
        log::logger().flush();
    }));
    log::info!(target: "agent", "starting");
    Ok(())
}

fn logger(dir: &Path, level: log::LevelFilter) -> Result<Logger> {
    let spec =
        format!("off,agent={level},network={level},clipboard={level},ipc={level},config={level}");
    Ok(Logger::try_with_str(spec)?
        .log_to_file(
            FileSpec::default()
                .directory(dir.join("logs"))
                .basename("agent")
                .suppress_timestamp(),
        )
        .format(format)
        .append()
        .rotate(
            Criterion::Size(FILE_SIZE),
            Naming::Numbers,
            Cleanup::KeepLogFiles(2),
        )
        .cleanup_in_background_thread(false)
        .write_mode(WriteMode::Direct))
}

fn format(out: &mut dyn Write, now: &mut DeferredNow, record: &log::Record) -> std::io::Result<()> {
    write!(
        out,
        "{} {:<5} {}: ",
        now.format("%Y-%m-%dT%H:%M:%S%.3f%:z"),
        record.level(),
        record.target()
    )?;
    // Keep a record on one line and bound individual records even for external errors.
    let message = record.args().to_string();
    for character in message.chars().take(2048) {
        if character.is_control() {
            write!(out, " ")?;
        } else {
            write!(out, "{character}")?;
        }
    }
    if message.chars().count() > 2048 {
        write!(out, "...")?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn log_rotation_is_bounded_and_restart_appends() {
        let dir = std::env::temp_dir().join(crate::core::engine::new_id());
        let (writer, handle) = logger(&dir, log::LevelFilter::Info)
            .unwrap()
            .rotate(
                Criterion::Size(256),
                Naming::Numbers,
                Cleanup::KeepLogFiles(2),
            )
            .build()
            .unwrap();
        for i in 0..40 {
            writer.log(
                &log::Record::builder()
                    .target("agent")
                    .level(log::Level::Info)
                    .args(format_args!("record {i}"))
                    .build(),
            );
        }
        writer.log(
            &log::Record::builder()
                .target("agent")
                .level(log::Level::Debug)
                .args(format_args!("hidden-debug"))
                .build(),
        );
        handle.shutdown();
        drop(writer);
        drop(handle);
        let files: Vec<_> = std::fs::read_dir(dir.join("logs"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        assert_eq!(files.len(), 3);
        for file in &files {
            assert!(std::fs::metadata(file).unwrap().len() < 400);
            assert!(
                !std::fs::read_to_string(file)
                    .unwrap()
                    .contains("hidden-debug")
            );
        }
        let current = dir.join("logs/agent_rCURRENT.log");
        let before = std::fs::read_to_string(&current).unwrap();
        let (writer, handle) = logger(&dir, log::LevelFilter::Info)
            .unwrap()
            .build()
            .unwrap();
        writer.log(
            &log::Record::builder()
                .target("agent")
                .level(log::Level::Info)
                .args(format_args!("restarted"))
                .build(),
        );
        handle.shutdown();
        drop(writer);
        drop(handle);
        let after = std::fs::read_to_string(&current).unwrap();
        assert!(after.starts_with(&before) && after.contains("restarted"));
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn records_are_timestamped_single_line_and_bounded() {
        let mut output = Vec::new();
        format(
            &mut output,
            &mut DeferredNow::new(),
            &log::Record::builder()
                .level(log::Level::Warn)
                .target("network")
                .args(format_args!("failed\nnext\r\x1b"))
                .build(),
        )
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains(" WARN  network: failed next  "));
        assert_eq!(&text[4..5], "-");
        assert_eq!(&text[10..11], "T");
        assert!(matches!(&text[23..24], "+" | "-"));
        assert!(!text.chars().any(char::is_control));
        let mut output = Vec::new();
        format(
            &mut output,
            &mut DeferredNow::new(),
            &log::Record::builder()
                .target("agent")
                .args(format_args!("{}", "x".repeat(4096)))
                .build(),
        )
        .unwrap();
        assert!(output.len() < 2200);
        assert!(output.ends_with(b"..."));
    }
}
