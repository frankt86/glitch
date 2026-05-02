fn main() {
    if std::env::var_os("CARGO_CFG_WINDOWS").is_some() {
        let mut res = winres::WindowsResource::new();
        res.set_icon("assets/app_icon.ico");
        res.compile().expect("failed to compile Windows resources");
    }
}
