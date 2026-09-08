//! Each invocation has its own cwd, so parallel tests exercise exactly the
//! bundler's filesystem semantics without process-global cwd races.
use serde_json::{json, Value};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

fn fixture() -> TempDir {
    let root = tempfile::tempdir().unwrap();
    for file in [
        "resources/retrieval/bundle/README.md",
        "templates/one.json",
        "templates/fonts/font.txt",
        "foreign-tree/retrieval/bundle/foreign.bin",
        "foreign/bundle",
        "unrelated/ok.md",
    ] {
        write(root.path(), file);
    }
    root
}

fn write(root: &Path, file: &str) {
    let file = root.join(file);
    fs::create_dir_all(file.parent().unwrap()).unwrap();
    fs::write(file, b"synthetic fixture").unwrap();
}

fn check(root: &Path, resources: Value, static_only: bool) -> Result<(), String> {
    let config = root.join("tauri.conf.json");
    fs::write(
        &config,
        serde_json::to_vec(&json!({"bundle": {"resources": resources}})).unwrap(),
    )
    .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_retrieval-package-contract"));
    if static_only {
        command.arg("--static");
    }
    let output = command.arg(config).output().unwrap();
    if output.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8(output.stderr).unwrap();
        assert!(
            !message.contains(&root.to_string_lossy().to_string()),
            "source path leaked"
        );
        Err(message)
    }
}

#[test]
fn permits_real_unrelated_lists_maps_and_readme_only_bundle() {
    let root = fixture();
    check(
        root.path(),
        json!([
            "templates/*.json",
            "templates/fonts/*",
            "resources/retrieval/bundle"
        ]),
        false,
    )
    .unwrap();
    check(
        root.path(),
        json!({"resources/retrieval/bundle":"resources/retrieval/bundle", "unrelated":"docs"}),
        false,
    )
    .unwrap();
    // Absolute unrelated sources are expanded by Tauri, not reinterpreted by us.
    let source = root.path().join("unrelated/*").display().to_string();
    check(
        root.path(),
        json!({"resources/retrieval/bundle":"resources/retrieval/bundle", source:"docs"}),
        false,
    )
    .unwrap();
}

#[test]
fn rejects_ancestor_sources_and_actual_tauri_glob_overlaps() {
    let root = fixture();
    for pattern in [
        "resources",
        "resources/**/*",
        "resources/**/bundle/*",
        "resources/[r]etrieval/bundle/*",
        "resources/[S-r]etrieval/bundle/*",
        "resources/[!z-a]etrieval/bundle/*",
        "resources/retrieval/bun?le/*",
    ] {
        let error = check(
            root.path(),
            json!(["resources/retrieval/bundle", pattern]),
            false,
        )
        .unwrap_err();
        assert!(
            error.contains("duplicate_bundle_source"),
            "{pattern}: {error}"
        );
    }
    assert!(check(
        root.path(),
        json!(["resources/retrieval/bundle", "."]),
        false
    )
    .is_err());
}

#[test]
fn preserves_tauri_star_trigger_and_case_sensitive_character_ranges() {
    let root = fixture();
    // Without '*', Tauri treats brackets as literal filename characters.
    write(root.path(), "resources/[S-r]etrieval/bundle/literal.txt");
    check(
        root.path(),
        json!([
            "resources/retrieval/bundle",
            "resources/[S-r]etrieval/bundle"
        ]),
        false,
    )
    .unwrap();
    // Uppercase-only class has a real unrelated match, but not lowercase retrieval.
    write(root.path(), "resources/Xetrieval/bundle/unrelated.txt");
    write(root.path(), "resources/Aetrieval/bundle/unrelated.txt");
    check(
        root.path(),
        json!([
            "resources/retrieval/bundle",
            "resources/[A-Z]etrieval/bundle/*"
        ]),
        false,
    )
    .unwrap();
    check(
        root.path(),
        json!([
            "resources/retrieval/bundle",
            "resources/[!S-r]etrieval/bundle/*"
        ]),
        false,
    )
    .unwrap();
    // A glob matching nothing is a Tauri build error, not a valid resource.
    assert!(check(
        root.path(),
        json!([
            "resources/retrieval/bundle",
            "resources/[z-a]etrieval/bundle/*"
        ]),
        false
    )
    .unwrap_err()
    .contains("resource_expansion_failed"));
}

#[test]
fn rejects_map_ancestor_injection_glob_flattening_and_root_file_collisions() {
    let root = fixture();
    for (source, target) in [
        ("foreign-tree", "resources"),
        ("foreign-tree/retrieval", "resources/retrieval"),
        (
            "foreign-tree/retrieval/bundle",
            "resources/retrieval/bundle",
        ),
        ("foreign/*", "resources/retrieval"),
        ("foreign/bundle", "resources"),
        ("foreign/bundle", "resources/retrieval"),
        ("templates/*.json", "resources/retrieval/bundle"),
    ] {
        let error = check(
            root.path(),
            json!({"resources/retrieval/bundle":"resources/retrieval/bundle", source:target}),
            false,
        )
        .unwrap_err();
        assert!(
            error.contains("overlapping_bundle_destination"),
            "{source} => {target}: {error}"
        );
    }
    write(
        root.path(),
        "foreign-root/resources/retrieval/bundle/foreign.bin",
    );
    assert!(check(
        root.path(),
        json!({"resources/retrieval/bundle":"resources/retrieval/bundle", "foreign-root":""}),
        false
    )
    .unwrap_err()
    .contains("overlapping_bundle_destination"));
}

#[test]
fn exact_mapping_cannot_be_omitted_duplicated_or_redirected() {
    let root = fixture();
    for resources in [
        json!([]),
        json!(["resources/retrieval/bundle", "resources/retrieval/bundle"]),
        json!({"resources/retrieval/bundle":"other"}),
        json!({"resources/retrieval/bundle":42}),
    ] {
        assert!(check(root.path(), resources, true).is_err());
    }
}

#[test]
fn rejects_empty_mapping_that_would_truncate_tauris_whole_list() {
    let root = fixture();
    fs::create_dir(root.path().join("empty-dir")).unwrap();
    let paths = vec![
        root.path().join("empty-dir").display().to_string(),
        root.path()
            .join("resources/retrieval/bundle")
            .display()
            .to_string(),
    ];
    assert!(
        tauri_utils::resources::ResourcePaths::new(&paths, true)
            .next()
            .is_none(),
        "pin Tauri's whole-list early termination"
    );
    assert!(check(
        root.path(),
        json!(["empty-dir", "resources/retrieval/bundle"]),
        false
    )
    .unwrap_err()
    .contains("empty_resource_mapping"));
}

#[test]
fn permits_static_crash_state_but_rechecks_globs_after_restore() {
    let root = fixture();
    let bundle = root.path().join("resources/retrieval/bundle");
    let backup = root
        .path()
        .join("resources/retrieval/.bundle-backup-selftest");
    fs::rename(&bundle, &backup).unwrap();
    let resources = json!(["resources/retrieval/bundle", "resources/**/*"]);
    check(root.path(), resources.clone(), true).unwrap();
    assert!(check(root.path(), resources.clone(), false).is_err());
    fs::rename(&backup, &bundle).unwrap();
    assert!(check(root.path(), resources, false)
        .unwrap_err()
        .contains("duplicate_bundle_source"));
    check(root.path(), json!(["resources/retrieval/bundle"]), false).unwrap();
}

#[cfg(windows)]
#[test]
fn rejects_root_and_glob_junction_aliases() {
    let root = fixture();
    let alias = root.path().join("bundle-alias");
    let bundle = root.path().join("resources/retrieval/bundle");
    let output = Command::new("powershell.exe").args(["-NoProfile", "-Command", "New-Item -ItemType Junction -Path $env:CONTRACT_TEST_ALIAS -Target $env:CONTRACT_TEST_BUNDLE | Out-Null"])
        .env("CONTRACT_TEST_ALIAS", &alias).env("CONTRACT_TEST_BUNDLE", &bundle).output().unwrap();
    assert!(
        output.status.success(),
        "junction fixture must actually be created"
    );
    for source in ["bundle-alias", "bundle-alias/*"] {
        let error = check(
            root.path(),
            json!({"resources/retrieval/bundle":"resources/retrieval/bundle", source:"other"}),
            false,
        )
        .unwrap_err();
        assert!(error.contains("reparse_resource_source"), "{error}");
    }
    // Remove only the junction itself. Its target remains intact.
    fs::remove_dir(alias).unwrap();
    assert!(bundle.join("README.md").is_file());
}
