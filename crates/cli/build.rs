fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/audioticon.ico");
        res.set("ProductName", "Audioticon Studio");
        res.set(
            "FileDescription",
            "Audioticon Studio — AI Audiobook & Drama Engine",
        );
        res.set("LegalCopyright", "Copyright (C) 2026 Jarosław Kuchta");
        let _ = res.compile();
    }
}
