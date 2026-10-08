/// Parses a CSV file into rows of strings.
pub fn read_rows(path: &Path) -> csv::Result<Vec<Vec<String>>> {
    let mut r = csv::Reader::from_path(path)?;
    r.records().map(|rec| rec.map(|r| r.iter().map(str::to_owned).collect())).collect()
}
