fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args[0] == "--help" {
        println!(
            "rustboard status|peers|pair <IP:port> --fingerprint <SHA256>|unpair <device>|pause|resume|mode text|mode all|doctor|version|stop"
        );
        return Ok(());
    }
    if args[0] == "version" {
        println!("rustboard {} protocol=1", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    #[cfg(windows)]
    {
        let value = rustboard_platform::windows::ipc::request(args)?;
        println!("{}", serde_json::to_string_pretty(&value)?);
        anyhow::ensure!(value["ok"] == true, "command failed");
        Ok(())
    }
    #[cfg(not(windows))]
    anyhow::bail!("Native IPC backend not installed");
}
