/// Retries a blocking GET with exponential backoff.
pub fn get_with_retry(url: &str, attempts: u32) -> Result<String, String> {
    for i in 0..attempts {
        match ureq::get(url).call() {
            Ok(r) => return r.into_string().map_err(|e| e.to_string()),
            Err(_) => std::thread::sleep(std::time::Duration::from_secs(1 << i)),
        }
    }
    Err("failed".into())
}
