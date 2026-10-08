use crate::prelude::*;

use atoman::fs;

const EXCLUDED_DIRS: &[&str] = &[
    "target",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "build",
    "dist",
    "out",
    ".next",
    ".nuxt",
    ".git",
    ".cargo",
    ".gradle",
    "bin",
    "obj",
];

#[cfg(target_os = "linux")]
pub async fn copy_directory_with_excludes(
    src_dir: &Path,
    dst_dir: &Path,
    tx: &Sender<Bytes>,
) -> Result<u64> {
    let mut count = 0u64;
    let mut stack = vec![(src_dir.to_path_buf(), dst_dir.to_path_buf())];

    while let Some((src, dst)) = stack.pop() {
        fs::create_dir_all(&dst).await?;
        let mut dir = fs::read_dir(&src).await?;

        while let Some(entry) = dir.next_entry().await? {
            let path = entry.path();
            let file_name = entry.file_name();
            let name_str = file_name.to_string_lossy();

            let file_type = match entry.file_type().await {
                Ok(ft) => ft,
                Err(_) => continue,
            };

            if file_type.is_dir() {
                if EXCLUDED_DIRS
                    .iter()
                    .any(|&ex| ex.eq_ignore_ascii_case(&name_str))
                {
                    continue;
                }
                stack.push((path, dst.join(file_name)));
            } else if file_type.is_file() {
                let rel_path = path.strip_prefix(src_dir).unwrap_or(&path);
                let _ = tx.send(Event::Thinking(format!(
                    "Backing up: {}",
                    rel_path.display()
                )));

                let target_file = dst.join(&file_name);
                fs::copy(&path, &target_file).await?;
                count += 1;
            }
        }
    }

    Ok(count)
}
