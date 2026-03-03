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
    assert!(paths
        .community_registry_cache()
        .ends_with("skills-community-registry"));
}
