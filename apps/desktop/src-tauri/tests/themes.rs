use course_workbench_lib::settings::AppSettings;
#[test]
fn old_settings_default_to_forest_and_three_themes_roundtrip() {
    let old: AppSettings = serde_json::from_str("{}").unwrap();
    assert_eq!(old.theme, "forest");
    for theme in ["forest", "paper", "night"] {
        let settings = AppSettings {
            theme: theme.into(),
            ..AppSettings::default()
        };
        assert!(settings.validate().is_ok());
        let saved = serde_json::to_string(&settings).unwrap();
        assert_eq!(
            serde_json::from_str::<AppSettings>(&saved).unwrap().theme,
            theme
        );
    }
}
#[test]
fn unknown_theme_is_rejected_before_saving() {
    let settings = AppSettings {
        theme: "unknown".into(),
        ..AppSettings::default()
    };
    assert!(settings
        .validate()
        .unwrap_err()
        .to_string()
        .contains("主题"));
}
