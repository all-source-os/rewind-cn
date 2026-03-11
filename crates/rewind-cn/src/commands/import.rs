use std::path::Path;

use rewind_cn_core::infrastructure::engine::RewindEngine;
use rewind_cn_core::infrastructure::importer::import_file;

const DATA_DIR: &str = ".rewind/data";

pub async fn execute(file: String, skip_closed: bool) -> Result<(), String> {
    if !Path::new(".rewind").exists() {
        return Err("No rewind project found. Run `rewind init` first.".into());
    }

    let path = Path::new(&file);
    if !path.exists() {
        return Err(format!("File not found: {file}"));
    }

    let engine = RewindEngine::load(DATA_DIR)
        .await
        .map_err(|e| e.to_string())?;

    let result = import_file(path, &engine, skip_closed).await?;

    println!(
        "Imported {} epic(s) and {} task(s){}.",
        result.epics_created,
        result.tasks_created,
        if result.skipped > 0 {
            format!(" ({} closed items skipped)", result.skipped)
        } else {
            String::new()
        }
    );

    Ok(())
}
