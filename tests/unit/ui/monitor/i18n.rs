use super::*;

#[test]
fn manual_toggle_defaults_to_english_and_covers_chinese() {
    assert_eq!(UiLanguage::default(), UiLanguage::En);
    assert_eq!(UiLanguage::En.toggle(), UiLanguage::ZhCn);
    assert_eq!(UiLanguage::ZhCn.toggle(), UiLanguage::En);
    assert_eq!(UiLanguage::ZhCn.tr("workspace"), "工作区");
}
