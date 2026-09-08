//! Build-time retrieval resource isolation. This crate has no application,
//! sidecar, model, or network dependency. Tauri itself expands the mappings.
use serde_json::Value;
use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
};
use tauri_utils::resources::{resource_relpath, ResourcePaths};

pub const BUNDLE: &str = "resources/retrieval/bundle";
pub type ContractResult<T> = Result<T, &'static str>;

fn path_key(path: &Path) -> Vec<String> {
    path.components()
        .filter_map(|part| match part {
            Component::CurDir => None,
            _ => {
                let value = part.as_os_str().to_string_lossy().into_owned();
                Some(if cfg!(windows) {
                    value.to_ascii_lowercase()
                } else {
                    value
                })
            }
        })
        .collect()
}

fn mappings(resources: &Value) -> ContractResult<Vec<(&str, Option<&str>)>> {
    match resources {
        Value::Array(items) => items
            .iter()
            .map(|item| {
                item.as_str()
                    .map(|s| (s, None))
                    .ok_or("invalid_resource_source")
            })
            .collect(),
        Value::Object(items) => items
            .iter()
            .map(|(source, target)| {
                target
                    .as_str()
                    .map(|s| (source.as_str(), Some(s)))
                    .ok_or("invalid_resource_target")
            })
            .collect(),
        _ => Err("invalid_resources"),
    }
}

fn owned(source: &str, target: Option<&str>) -> bool {
    path_key(Path::new(source)) == path_key(Path::new(BUNDLE))
        && path_key(&resource_relpath(Path::new(target.unwrap_or(source))))
            == path_key(Path::new(BUNDLE))
}

/// Safe before crash recovery: validates configuration without requiring the
/// owned directory to exist. A sole verified backup may still need restoration.
pub fn validate_static(resources: &Value) -> ContractResult<()> {
    let entries = mappings(resources)?;
    if entries.iter().any(|(source, _)| source.is_empty()) {
        return Err("empty_resource_source");
    }
    if entries
        .iter()
        .filter(|(source, target)| owned(source, *target))
        .count()
        != 1
    {
        return Err("expected_one_exact_bundle_mapping");
    }
    Ok(())
}

fn assert_no_links(path: &Path) -> ContractResult<()> {
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|_| "source_unreadable")?
            .join(path)
    };
    let mut ancestor = PathBuf::new();
    for component in absolute.components() {
        ancestor.push(component.as_os_str());
        if matches!(component, Component::Prefix(_) | Component::RootDir) {
            continue;
        }
        let metadata = fs::symlink_metadata(&ancestor).map_err(|_| "source_unreadable")?;
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err("reparse_resource_source");
            }
        }
        if metadata.file_type().is_symlink() {
            return Err("symlink_resource_source");
        }
    }
    Ok(())
}

/// Run with the application directory as cwd, exactly as Tauri does. Call only
/// after verified crash recovery, and again after publication. The stager's
/// separate integrity gate owns the contents of the one approved directory.
pub fn validate_expanded(resources: &Value) -> ContractResult<usize> {
    validate_static(resources)?;
    assert_no_links(Path::new(BUNDLE))?;
    let bundle = fs::canonicalize(BUNDLE).map_err(|_| "bundle_missing")?;
    if !bundle.is_dir() {
        return Err("bundle_not_directory");
    }
    let bundle_source_key = path_key(&bundle);
    let bundle_target_key = path_key(Path::new(BUNDLE));
    let mut checked = 0;
    for (source, target) in mappings(resources)? {
        if owned(source, target) {
            continue;
        }
        let before = checked;
        let list = vec![source.to_owned()];
        let map: HashMap<String, String> = target
            .into_iter()
            .map(|dest| (source.to_owned(), dest.to_owned()))
            .collect();
        let expanded = if target.is_some() {
            ResourcePaths::from_map(&map, true)
        } else {
            ResourcePaths::new(&list, true)
        };
        for item in expanded.iter() {
            let item = item.map_err(|_| "resource_expansion_failed")?;
            assert_no_links(item.path())?;
            let canonical = fs::canonicalize(item.path()).map_err(|_| "source_unreadable")?;
            if path_key(&canonical).starts_with(&bundle_source_key) {
                return Err("duplicate_bundle_source");
            }
            let destination = path_key(item.target());
            // A file at an ancestor (e.g. resources/retrieval) also collides.
            if destination.starts_with(&bundle_target_key)
                || bundle_target_key.starts_with(&destination)
            {
                return Err("overlapping_bundle_destination");
            }
            checked += 1;
        }
        // Tauri's outer iterator stops entirely on an empty literal directory;
        // accepting one could silently omit subsequent resources including ours.
        if checked == before {
            return Err("empty_resource_mapping");
        }
    }
    Ok(checked)
}
