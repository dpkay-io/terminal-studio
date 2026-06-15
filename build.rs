fn main() {
    #[cfg(target_os = "windows")]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        res.set("ProductName", "Terminal Studio");
        res.set("FileDescription", "GPU-accelerated terminal multiplexer");
        res.set(
            "LegalCopyright",
            "Copyright \u{00a9} Terminal Studio contributors. Apache-2.0 License.",
        );
        res.set("OriginalFilename", "terminal-studio.exe");
        if let Err(e) = res.compile() {
            eprintln!("cargo:warning=Failed to compile Windows resources: {e}");
        }
    }
}
