use super::*;

#[test]
fn manual_toggle_defaults_to_english_and_covers_chinese() {
    assert_eq!(UiLanguage::default(), UiLanguage::En);
    assert_eq!(UiLanguage::En.toggle(), UiLanguage::ZhCn);
    assert_eq!(UiLanguage::ZhCn.toggle(), UiLanguage::En);
    assert_eq!(UiLanguage::ZhCn.tr("workspace"), "工作区");
}

#[test]
fn every_literal_key_used_by_the_monitor_has_a_chinese_translation() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ui/monitor");
    let mut used = std::collections::BTreeSet::new();
    for entry in std::fs::read_dir(&dir).unwrap() {
        let path = entry.unwrap().path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("rs") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        let mut rest = source.as_str();
        while let Some(index) = rest.find(".tr(\"") {
            let after = &rest[index + 5..];
            let Some(end) = after.find('"') else { break };
            used.insert(after[..end].to_owned());
            rest = &after[end..];
        }
    }
    assert!(used.len() > 100, "the sweep found only {} keys", used.len());
    let untranslated: Vec<String> = used
        .iter()
        .filter(|key| UiLanguage::ZhCn.tr(key) == key.as_str())
        .cloned()
        .collect();
    assert!(
        untranslated.is_empty(),
        "missing Chinese translations: {untranslated:?}"
    );
}
