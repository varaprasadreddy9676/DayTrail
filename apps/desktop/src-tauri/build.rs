fn main() {
    let build_unix = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=DAYTRAIL_BUILD_UNIX={build_unix}");
    tauri_build::build();
}
