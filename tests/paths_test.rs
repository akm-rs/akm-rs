use tempfile::TempDir;

#[test]
fn paths_from_roots_derives_all_dirs() {
    let tmp = TempDir::new().unwrap();
    let paths = akm::paths::Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );

    assert!(paths.data_dir().ends_with("akm"));
    assert!(paths.config_dir().ends_with("akm"));
    assert!(paths.cache_dir().ends_with("akm"));
    assert!(paths.library_json().ends_with("library.json"));
    assert!(paths.config_file().ends_with("config.toml"));
}

/// Everything the registry owns sits under `library/`; everything AKM owns
/// sits beside it, so a clone can never clobber AKM's own state.
#[test]
fn registry_tree_and_akm_owned_files_are_siblings() {
    let tmp = TempDir::new().unwrap();
    let paths = akm::paths::Paths::from_roots(
        &tmp.path().join("data"),
        &tmp.path().join("config"),
        &tmp.path().join("cache"),
        tmp.path(),
    );

    let library = paths.library_dir();
    assert_eq!(library, paths.data_dir().join("library"));

    for inside in [
        paths.skills_dir(),
        paths.agents_dir(),
        paths.library_json(),
        paths.instructions_file(),
    ] {
        assert!(inside.starts_with(&library), "{} escaped", inside.display());
    }

    for outside in [paths.local_json(), paths.tools_json(), paths.shell_init()] {
        assert!(
            !outside.starts_with(&library),
            "{} must stay outside the registry tree",
            outside.display()
        );
    }
}
