//! File-hash trees for isolation evidence. A model saying "I cannot write" is not evidence.
//! Symlinks are recorded by their target and never followed.

use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io;
use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Entry {
    File(String),
    Link(String),
    Dir,
}

pub type Tree = BTreeMap<String, Entry>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added(String),
    Removed(String),
    Modified(String),
}

pub fn hash_tree(root: &Path) -> io::Result<Tree> {
    let mut tree = Tree::new();
    walk(root, root, &mut tree)?;
    Ok(tree)
}

fn walk(root: &Path, dir: &Path, tree: &mut Tree) -> io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let path = e?.path();
        let rel = path
            .strip_prefix(root)
            .expect("under root")
            .to_string_lossy()
            .into_owned();
        let meta = std::fs::symlink_metadata(&path)?;
        if meta.file_type().is_symlink() {
            tree.insert(
                rel,
                Entry::Link(std::fs::read_link(&path)?.to_string_lossy().into_owned()),
            );
        } else if meta.is_dir() {
            tree.insert(rel, Entry::Dir);
            walk(root, &path, tree)?;
        } else {
            let digest = Sha256::digest(std::fs::read(&path)?);
            tree.insert(rel, Entry::File(format!("{digest:x}")));
        }
    }
    Ok(())
}

pub fn diff(before: &Tree, after: &Tree) -> Vec<Change> {
    let keys: std::collections::BTreeSet<&String> = before.keys().chain(after.keys()).collect();
    keys.into_iter()
        .filter_map(|k| match (before.get(k), after.get(k)) {
            (Some(a), Some(b)) if a != b => Some(Change::Modified(k.clone())),
            (Some(_), None) => Some(Change::Removed(k.clone())),
            (None, Some(_)) => Some(Change::Added(k.clone())),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> std::path::PathBuf {
        let d = std::env::temp_dir().join(format!("bff-fshash-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&d);
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn hashes_files_and_dirs_with_relative_paths() {
        let d = scratch("basic");
        fs::create_dir_all(d.join("sub")).unwrap();
        fs::write(d.join("a.txt"), "hello").unwrap();
        fs::write(d.join("sub/b.txt"), "x").unwrap();
        let t = hash_tree(&d).unwrap();
        assert_eq!(
            t["a.txt"],
            Entry::File("2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824".into())
        );
        assert_eq!(t["sub"], Entry::Dir);
        assert!(t.contains_key("sub/b.txt"));
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn symlinks_are_recorded_not_followed() {
        let d = scratch("link");
        let outside = scratch("link-outside");
        fs::write(outside.join("secret.txt"), "s").unwrap();
        std::os::unix::fs::symlink(&outside, d.join("escape")).unwrap();
        let t = hash_tree(&d).unwrap();
        assert_eq!(
            t["escape"],
            Entry::Link(outside.to_string_lossy().into_owned())
        );
        assert!(!t.keys().any(|k| k.contains("secret")));
        fs::remove_dir_all(&d).unwrap();
        fs::remove_dir_all(&outside).unwrap();
    }

    #[test]
    fn diff_reports_added_removed_and_modified() {
        let d = scratch("diff");
        fs::write(d.join("keep.txt"), "1").unwrap();
        fs::write(d.join("gone.txt"), "1").unwrap();
        fs::write(d.join("edit.txt"), "1").unwrap();
        let before = hash_tree(&d).unwrap();
        fs::remove_file(d.join("gone.txt")).unwrap();
        fs::write(d.join("edit.txt"), "2").unwrap();
        fs::write(d.join("new.txt"), "n").unwrap();
        let after = hash_tree(&d).unwrap();
        assert_eq!(
            diff(&before, &after),
            vec![
                Change::Modified("edit.txt".into()),
                Change::Removed("gone.txt".into()),
                Change::Added("new.txt".into())
            ]
        );
        assert!(diff(&after, &after).is_empty());
        fs::remove_dir_all(&d).unwrap();
    }

    #[test]
    fn missing_root_is_an_error() {
        assert!(hash_tree(Path::new("/nonexistent/bff-xyz")).is_err());
    }
}
