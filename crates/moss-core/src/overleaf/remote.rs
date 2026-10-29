//! The real Overleaf behind the sync engine's [`Remote`] interface.
//!
//! Reads are one zip download. Writes need Overleaf's ids for folders and files, which only the
//! realtime channel hands out, so the tree is read once, kept, and updated after each write.
//! None of the write endpoints have been run against a live account yet (ROADMAP M0).

use std::collections::BTreeMap;
use std::sync::Mutex;

use super::{Client, RemoteKind, RemoteTree};
use crate::sync::fileset::FileSet;
use crate::sync::remote::{Fetched, Remote};
use crate::{Error, Result};

#[derive(Debug, Clone)]
struct Node {
    id: String,
    kind: RemoteKind,
}

struct Tree {
    root_folder_id: String,
    /// Every entry by path, folders included.
    nodes: BTreeMap<String, Node>,
}

impl Tree {
    fn from(remote: RemoteTree) -> Tree {
        Tree {
            root_folder_id: remote.root_folder_id,
            nodes: remote
                .entries
                .into_iter()
                .map(|e| (e.path, Node { id: e.id, kind: e.kind }))
                .collect(),
        }
    }
}

pub struct OverleafRemote {
    client: Client,
    project_id: String,
    tree: Mutex<Option<Tree>>,
}

impl OverleafRemote {
    pub fn new(client: Client, project_id: impl Into<String>) -> OverleafRemote {
        OverleafRemote {
            client,
            project_id: project_id.into(),
            tree: Mutex::new(None),
        }
    }

    /// Run `f` with the project tree, reading it first if this is the first write.
    fn with_tree<T>(&self, f: impl FnOnce(&mut Tree) -> Result<T>) -> Result<T> {
        let mut guard = self.tree.lock().unwrap();
        if guard.is_none() {
            *guard = Some(Tree::from(self.client.project_tree(&self.project_id)?));
        }
        f(guard.as_mut().expect("just filled"))
    }

    /// The id of the folder at `dir` (`""` is the root), creating missing folders on the way.
    fn ensure_folder(&self, tree: &mut Tree, dir: &str) -> Result<String> {
        let mut parent = tree.root_folder_id.clone();
        let mut path = String::new();
        for name in dir.split('/').filter(|c| !c.is_empty()) {
            if !path.is_empty() {
                path.push('/');
            }
            path.push_str(name);
            parent = match tree.nodes.get(&path) {
                Some(node) if node.kind == RemoteKind::Folder => node.id.clone(),
                Some(_) => {
                    return Err(Error::Overleaf(format!(
                        "`{path}` is a file on Overleaf, so a folder of that name cannot be made"
                    )));
                }
                None => {
                    let id = self.client.create_folder(&self.project_id, &parent, name)?;
                    tree.nodes.insert(path.clone(), Node { id: id.clone(), kind: RemoteKind::Folder });
                    id
                }
            };
        }
        Ok(parent)
    }
}

impl Remote for OverleafRemote {
    fn fetch(&self) -> Result<Fetched> {
        // Whatever the tree looked like before, it is out of date now.
        *self.tree.lock().unwrap() = None;
        let zip = self.client.download_zip(&self.project_id)?;
        let (files, skipped) = FileSet::from_zip(&zip)?;
        Ok(Fetched { files, skipped })
    }

    fn upload(&self, path: &str, bytes: &[u8]) -> Result<()> {
        let (dir, name) = split(path);
        self.with_tree(|tree| {
            let folder = self.ensure_folder(tree, dir)?;
            // Overleaf replaces a same-named file or doc, and swaps its type if the content now
            // belongs to the other kind.
            let uploaded = self.client.upload_file(&self.project_id, &folder, name, bytes.to_vec())?;
            let kind = if uploaded.entity_type == "file" { RemoteKind::File } else { RemoteKind::Doc };
            tree.nodes.insert(path.to_string(), Node { id: uploaded.entity_id, kind });
            Ok(())
        })
    }

    fn delete(&self, path: &str) -> Result<()> {
        self.with_tree(|tree| {
            let node = tree
                .nodes
                .get(path)
                .cloned()
                .ok_or_else(|| Error::Overleaf(format!("`{path}` is not in the Overleaf project")))?;
            self.client.delete_entity(&self.project_id, node.kind, &node.id)?;
            tree.nodes.remove(path);
            Ok(())
        })
    }

    fn rename(&self, from: &str, to: &str) -> Result<()> {
        let ((from_dir, from_name), (to_dir, to_name)) = (split(from), split(to));
        self.with_tree(|tree| {
            let node = tree
                .nodes
                .get(from)
                .cloned()
                .ok_or_else(|| Error::Overleaf(format!("`{from}` is not in the Overleaf project")))?;
            if tree.nodes.contains_key(to) {
                return Err(Error::Overleaf(format!("`{to}` already exists on Overleaf")));
            }
            if from_dir != to_dir {
                let folder = self.ensure_folder(tree, to_dir)?;
                self.client.move_entity(&self.project_id, node.kind, &node.id, &folder)?;
            }
            if from_name != to_name {
                self.client.rename_entity(&self.project_id, node.kind, &node.id, to_name)?;
            }
            tree.nodes.remove(from);
            tree.nodes.insert(to.to_string(), node);
            Ok(())
        })
    }
}

/// `"a/b/c.tex"` into `("a/b", "c.tex")`.
fn split(path: &str) -> (&str, &str) {
    path.rsplit_once('/').unwrap_or(("", path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_a_path_into_folder_and_name() {
        assert_eq!(split("main.tex"), ("", "main.tex"));
        assert_eq!(split("a/b/c.tex"), ("a/b", "c.tex"));
    }
}
