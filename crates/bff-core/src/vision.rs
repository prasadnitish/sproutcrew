//! Pictures for models that can see them. A step's picture sources are copied into its own folder (at most
//! [`MAX_IMAGES`], each at most [`MAX_IMAGE_BYTES`]), and an API route whose model reads images attaches them to
//! the request. The pictures are untrusted data like every source; their text was already read by character
//! recognition and checked for instructions when they were added. Each picture adds [`TOKENS_PER_IMAGE`] to the
//! request's worst case, so the limits still hold before anything is sent.

use std::path::Path;

pub const MAX_IMAGES: usize = 4;
pub const MAX_IMAGE_BYTES: u64 = 4 * 1024 * 1024;
/// A conservative token count for one picture, used only for the worst case held before sending.
pub const TOKENS_PER_IMAGE: u64 = 2_000;
const DIR: &str = "images";

fn mime(ext: &str) -> Option<&'static str> {
    Some(match ext {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        _ => return None,
    })
}

/// Copies the project's picture sources into `task_dir/images`, and returns how many.
pub fn stage_images(root: &Path, task_dir: &Path) -> Result<usize, String> {
    let store = crate::sources::SourceStore::open(root)?;
    let mut n = 0;
    for s in store.list() {
        if n >= MAX_IMAGES {
            break;
        }
        if mime(&s.ext).is_none() || s.bytes > MAX_IMAGE_BYTES {
            continue;
        }
        let Some(original) = store.original_path(&s.id) else {
            continue;
        };
        let dir = task_dir.join(DIR);
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        std::fs::copy(&original, dir.join(format!("{}.{}", s.id, s.ext)))
            .map_err(|e| e.to_string())?;
        n += 1;
    }
    Ok(n)
}

/// The pictures in a step's folder as `data:` URLs, in name order.
pub fn data_urls(task_dir: &Path) -> Vec<String> {
    use base64::Engine;
    let Ok(rd) = std::fs::read_dir(task_dir.join(DIR)) else {
        return vec![];
    };
    let mut paths: Vec<_> = rd.flatten().map(|e| e.path()).collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|p| {
            let ext = p.extension()?.to_str()?.to_ascii_lowercase();
            let m = mime(&ext)?;
            let meta = std::fs::symlink_metadata(&p).ok()?;
            if !meta.is_file() || meta.len() > MAX_IMAGE_BYTES {
                return None;
            }
            let b = std::fs::read(&p).ok()?;
            Some(format!(
                "data:{m};base64,{}",
                base64::engine::general_purpose::STANDARD.encode(b)
            ))
        })
        .take(MAX_IMAGES)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picture_sources_are_copied_for_the_step_and_read_back_as_data_urls_up_to_the_limit() {
        let root = std::env::temp_dir().join(format!("bff-vision-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let task = root.join("tasks/t1");
        std::fs::create_dir_all(&task).unwrap();
        assert_eq!(stage_images(&root, &task).unwrap(), 0);
        assert!(data_urls(&task).is_empty());
        std::fs::create_dir_all(task.join(DIR)).unwrap();
        std::fs::write(task.join(DIR).join("a.png"), [137, 80, 78, 71]).unwrap();
        std::fs::write(task.join(DIR).join("b.txt"), "not a picture").unwrap();
        let urls = data_urls(&task);
        assert_eq!(urls, ["data:image/png;base64,iVBORw=="]);
        std::fs::remove_dir_all(&root).unwrap();
    }
}
