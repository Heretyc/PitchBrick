fn main() {
    #[cfg(windows)]
    {
        let mut res = winres::WindowsResource::new();
        res.set_icon("docs/logo.ico");
        res.compile().expect("failed to compile Windows resources");
    }
}
