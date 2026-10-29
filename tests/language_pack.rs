use purple_ssh::i18n;
#[test]
fn switch_languages_and_preserve_user_values() {
    let _guard = purple_ssh::demo_flag::GLOBAL_TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let language = dir.path().join("language");
    std::fs::create_dir(dir.path().join("locales")).unwrap();
    std::fs::write(
        dir.path().join("locales/zh-CN.json"),
        include_str!("../locales/zh-CN.json"),
    )
    .unwrap();
    i18n::initialize(Some("en"), Some(language.clone())).unwrap();
    assert_eq!(i18n::text("Password Source"), "Password Source");
    i18n::save("zh-CN").unwrap();
    assert_eq!(i18n::text("Password Source"), "密码来源");
    assert_eq!(i18n::text("Snippets"), "命令片段");
    assert_eq!(i18n::text(" connect "), " 连接 ");
    assert_eq!(
        i18n::text("UNTRANSLATED_TECHNICAL_STRING"),
        "UNTRANSLATED_TECHNICAL_STRING"
    );
    let config = "~/.ssh/custom-config";
    assert_eq!(
        purple_ssh::messages::key_push_no_hosts(config),
        format!("{config} 中没有主机。请先添加主机，再回到这里。")
    );
    let value = "prod-{secret}-中文";
    assert_eq!(
        i18n::format_text("Edit: {}", &[value.into()]),
        format!("编辑：{value}")
    );
    assert_eq!(std::fs::read_to_string(&language).unwrap(), "zh-CN\n");
    let mut app = purple_ssh::demo::build_demo_app();
    let mut anim = purple_ssh::animation::AnimationState::new();
    let mut terminal = ratatui::Terminal::new(ratatui::backend::TestBackend::new(120, 40)).unwrap();
    terminal
        .draw(|frame| purple_ssh::ui::render(frame, &mut app, &mut anim))
        .unwrap();
    let mut screen = String::new();
    for row in terminal.backend().buffer().content.chunks(120) {
        let mut x = 0;
        while x < row.len() {
            let symbol = row[x].symbol();
            screen.push_str(symbol);
            x += unicode_width::UnicodeWidthStr::width(symbol).max(1);
        }
        screen.push('\n');
    }
    assert!(screen.contains("主机"), "Chinese navigation missing");
    assert!(
        screen.contains("命令片段"),
        "Chinese snippets navigation missing"
    );
    assert!(
        screen.contains("prod-eu1"),
        "Host alias must remain unchanged"
    );
    // A demo toggle must never persist a preference.
    i18n::toggle_for_session(false).unwrap();
    assert_eq!(i18n::text("Snippets"), "Snippets");
    assert_eq!(std::fs::read_to_string(&language).unwrap(), "zh-CN\n");
    i18n::toggle_for_session(false).unwrap();
    i18n::toggle().unwrap();
    assert_eq!(i18n::text("Snippets"), "Snippets");
    assert_eq!(std::fs::read_to_string(&language).unwrap(), "en\n");
    i18n::initialize(Some("en"), None).unwrap();
    i18n::toggle_for_session(true).unwrap();
    assert!(i18n::is_chinese());
    assert_eq!(std::fs::read_to_string(&language).unwrap(), "en\n");
    i18n::initialize(None, None).unwrap();
}
