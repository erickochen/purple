use super::*;

#[test]
fn color_slot_resolves_truecolor() {
    let slot = ColorSlot {
        truecolor: Some(Color::Rgb(147, 51, 234)),
        ansi16: Some(Color::Magenta),
        add_modifier: None,
        remove_modifier: None,
    };
    let style = slot.to_style(2);
    assert_eq!(style.fg, Some(Color::Rgb(147, 51, 234)));
}

#[test]
fn color_slot_resolves_ansi16() {
    let slot = ColorSlot {
        truecolor: Some(Color::Rgb(147, 51, 234)),
        ansi16: Some(Color::Magenta),
        add_modifier: None,
        remove_modifier: None,
    };
    let style = slot.to_style(1);
    assert_eq!(style.fg, Some(Color::Magenta));
}

#[test]
fn color_slot_resolves_no_color() {
    let slot = ColorSlot {
        truecolor: Some(Color::Rgb(147, 51, 234)),
        ansi16: Some(Color::Magenta),
        add_modifier: Some(Modifier::BOLD),
        remove_modifier: None,
    };
    let style = slot.to_style(0);
    assert_eq!(style.fg, None);
    assert!(style.add_modifier.contains(Modifier::BOLD));
}

#[test]
fn color_slot_remove_modifier() {
    let slot = ColorSlot {
        truecolor: None,
        ansi16: None,
        add_modifier: Some(Modifier::BOLD),
        remove_modifier: Some(Modifier::DIM),
    };
    let style = slot.to_style(0);
    assert!(style.add_modifier.contains(Modifier::BOLD));
    assert!(style.sub_modifier.contains(Modifier::DIM));
}

#[test]
fn color_slot_bg_resolves_truecolor() {
    let slot = ColorSlot {
        truecolor: Some(Color::Rgb(147, 51, 234)),
        ansi16: Some(Color::Magenta),
        add_modifier: None,
        remove_modifier: None,
    };
    let style = slot.to_style_bg(2);
    assert_eq!(style.bg, Some(Color::Rgb(147, 51, 234)));
    assert_eq!(style.fg, None);
}

#[test]
fn theme_def_purple_default_accent() {
    let t = ThemeDef::purple();
    assert_eq!(t.accent.truecolor, Some(Color::Rgb(147, 51, 234)));
}

#[test]
fn theme_error_returns_bold_red_truecolor() {
    let _lock = TEST_MUTEX.lock().unwrap();
    init_with_mode(2);
    let style = error();
    assert_eq!(style.fg, Some(Color::Rgb(239, 68, 68)));
    assert!(style.add_modifier.contains(Modifier::BOLD));
    COLOR_MODE.store(1, Ordering::Release);
}

#[test]
fn theme_selected_row_removes_dim() {
    let _lock = TEST_MUTEX.lock().unwrap();
    init_with_mode(2);
    set_theme(ThemeDef::purple());
    let style = selected_row();
    assert!(style.sub_modifier.contains(Modifier::DIM));
    assert_eq!(style.bg, Some(Color::Rgb(147, 51, 234)));
    assert_eq!(style.fg, Some(Color::White));
    COLOR_MODE.store(1, Ordering::Release);
    set_theme(ThemeDef::purple());
}

#[test]
fn theme_no_color_mode_ignores_colors() {
    let _lock = TEST_MUTEX.lock().unwrap();
    init_with_mode(0);
    let style = error();
    assert_eq!(style.fg, None);
    COLOR_MODE.store(1, Ordering::Release);
}

#[test]
fn all_builtin_themes_have_unique_names() {
    let themes = ThemeDef::builtins();
    let mut names: Vec<&str> = themes.iter().map(|t| t.name.as_str()).collect();
    names.sort();
    names.dedup();
    assert_eq!(names.len(), themes.len());
}

#[test]
fn all_builtin_themes_have_required_slots() {
    for t in ThemeDef::builtins() {
        if t.name != "No Color" {
            assert!(
                t.accent.truecolor.is_some(),
                "{} accent missing truecolor",
                t.name
            );
            assert!(
                t.error.truecolor.is_some(),
                "{} error missing truecolor",
                t.name
            );
        }
    }
}

#[test]
fn builtin_count_is_11() {
    assert_eq!(ThemeDef::builtins().len(), 11);
}

#[test]
fn no_color_theme_has_no_truecolor() {
    let t = ThemeDef::no_color();
    assert!(t.accent.truecolor.is_none());
    assert!(t.error.truecolor.is_none());
    assert!(t.success.truecolor.is_none());
}

#[test]
fn find_builtin_case_insensitive() {
    assert!(ThemeDef::find_builtin("catppuccin mocha").is_some());
    assert!(ThemeDef::find_builtin("CATPPUCCIN MOCHA").is_some());
    assert!(ThemeDef::find_builtin("nonexistent").is_none());
}

#[test]
fn parse_custom_theme_valid() {
    let toml = "name = \"My Theme\"\n\
                 accent = \"#ff0000\"\n\
                 success = \"#00ff00\"\n\
                 warning = \"#ffff00\"\n\
                 error = \"#ff0000\"\n";
    let t = ThemeDef::parse_toml(toml).unwrap();
    assert_eq!(t.name, "My Theme");
    assert_eq!(t.accent.truecolor, Some(Color::Rgb(255, 0, 0)));
    assert_eq!(t.success.truecolor, Some(Color::Rgb(0, 255, 0)));
    assert_eq!(t.warning.truecolor, Some(Color::Rgb(255, 255, 0)));
    assert_eq!(t.error.truecolor, Some(Color::Rgb(255, 0, 0)));
}

#[test]
fn parse_custom_theme_partial_fills_from_purple() {
    let toml = "name = \"Partial\"\n\
                 accent = \"#ff0000\"\n";
    let t = ThemeDef::parse_toml(toml).unwrap();
    let purple = ThemeDef::purple();
    assert_eq!(t.accent.truecolor, Some(Color::Rgb(255, 0, 0)));
    // success should fall back to Purple default
    assert_eq!(t.success.truecolor, purple.success.truecolor);
}

#[test]
fn parse_custom_theme_invalid_hex_skipped() {
    let toml = "name = \"BadHex\"\n\
                 accent = \"not-hex\"\n";
    let t = ThemeDef::parse_toml(toml).unwrap();
    let purple = ThemeDef::purple();
    // Falls back to Purple default since parse_hex fails
    assert_eq!(t.accent.truecolor, purple.accent.truecolor);
}

#[test]
fn parse_custom_theme_missing_name() {
    let toml = "accent = \"#ff0000\"\n";
    assert!(ThemeDef::parse_toml(toml).is_none());
}

#[test]
fn parse_hex_color_valid() {
    assert_eq!(parse_hex("#ff0000"), Some(Color::Rgb(255, 0, 0)));
}

#[test]
fn parse_hex_color_invalid() {
    assert!(parse_hex("not-hex").is_none());
    assert!(parse_hex("#gg0000").is_none());
    assert!(parse_hex("#fff").is_none());
}

#[test]
fn parse_ansi_color_name() {
    assert_eq!(parse_ansi_name("Red"), Some(Color::Red));
    assert_eq!(parse_ansi_name("blue"), Some(Color::Blue));
    assert_eq!(parse_ansi_name("DarkGray"), Some(Color::DarkGray));
    assert_eq!(parse_ansi_name("dark_gray"), Some(Color::DarkGray));
    assert_eq!(parse_ansi_name("unknown"), None);
}

// --- auto_ansi16 tests ---

#[test]
fn auto_ansi16_black_for_low_luminance() {
    assert_eq!(auto_ansi16(Color::Rgb(10, 10, 10)), Some(Color::Black));
}

#[test]
fn auto_ansi16_bright_red() {
    assert_eq!(auto_ansi16(Color::Rgb(200, 50, 30)), Some(Color::LightRed));
}

#[test]
fn auto_ansi16_dark_red() {
    assert_eq!(auto_ansi16(Color::Rgb(150, 50, 30)), Some(Color::Red));
}

#[test]
fn auto_ansi16_bright_green() {
    assert_eq!(
        auto_ansi16(Color::Rgb(50, 200, 30)),
        Some(Color::LightGreen)
    );
}

#[test]
fn auto_ansi16_bright_blue() {
    assert_eq!(auto_ansi16(Color::Rgb(30, 50, 200)), Some(Color::LightBlue));
}

#[test]
fn auto_ansi16_yellow() {
    assert_eq!(auto_ansi16(Color::Rgb(200, 200, 50)), Some(Color::Yellow));
}

#[test]
fn auto_ansi16_magenta() {
    assert_eq!(auto_ansi16(Color::Rgb(200, 50, 200)), Some(Color::Magenta));
}

#[test]
fn auto_ansi16_cyan() {
    assert_eq!(auto_ansi16(Color::Rgb(50, 200, 200)), Some(Color::Cyan));
}

#[test]
fn auto_ansi16_white() {
    assert_eq!(auto_ansi16(Color::Rgb(230, 230, 230)), Some(Color::White));
}

#[test]
fn auto_ansi16_gray() {
    assert_eq!(auto_ansi16(Color::Rgb(120, 120, 120)), Some(Color::Gray));
}

#[test]
fn auto_ansi16_passthrough_non_rgb() {
    assert_eq!(auto_ansi16(Color::Red), Some(Color::Red));
}

// --- find_builtin canonical name test ---

#[test]
fn find_builtin_returns_canonical_name() {
    let theme = ThemeDef::find_builtin("catppuccin mocha").unwrap();
    assert_eq!(theme.name, "Catppuccin Mocha");
}

// --- parse_toml edge case tests ---

#[test]
fn parse_toml_inline_comment() {
    let toml = "name = \"Commented\"\naccent = \"#ff0000\" # brand color\n";
    let theme = ThemeDef::parse_toml(toml).unwrap();
    assert_eq!(theme.accent.truecolor, Some(Color::Rgb(255, 0, 0)));
}

#[test]
fn parse_toml_ansi_override() {
    let toml = "name = \"Ansi\"\naccent = \"#ff0000\"\naccent_ansi = \"Blue\"\n";
    let theme = ThemeDef::parse_toml(toml).unwrap();
    assert_eq!(theme.accent.ansi16, Some(Color::Blue));
}

#[test]
fn parse_toml_duplicate_keys_last_wins() {
    let toml = "name = \"First\"\nname = \"Second\"\naccent = \"#ff0000\"\n";
    let theme = ThemeDef::parse_toml(toml).unwrap();
    assert_eq!(theme.name, "Second");
}

#[test]
fn parse_toml_footer_key_bg_maps_to_footer_key() {
    let toml = "name = \"Footer\"\nfooter_key_bg = \"#112233\"\n";
    let theme = ThemeDef::parse_toml(toml).unwrap();
    assert_eq!(theme.footer_key.truecolor, Some(Color::Rgb(17, 34, 51)));
}

#[test]
fn parse_toml_badge_bg_maps_to_badge() {
    let toml = "name = \"Badge\"\nbadge_bg = \"#aabbcc\"\n";
    let theme = ThemeDef::parse_toml(toml).unwrap();
    assert_eq!(theme.badge.truecolor, Some(Color::Rgb(170, 187, 204)));
}

#[test]
fn parse_toml_ignores_unknown_keys() {
    let toml = "name = \"Unknown\"\nunknown_key = \"value\"\naccent = \"#ff0000\"\n";
    let theme = ThemeDef::parse_toml(toml).unwrap();
    assert_eq!(theme.accent.truecolor, Some(Color::Rgb(255, 0, 0)));
}

#[test]
fn parse_toml_empty_lines_and_comments() {
    let toml =
        "\n# This is a comment\n\nname = \"Spaced\"\n\n# accent color\naccent = \"#00ff00\"\n";
    let theme = ThemeDef::parse_toml(toml).unwrap();
    assert_eq!(theme.name, "Spaced");
    assert_eq!(theme.accent.truecolor, Some(Color::Rgb(0, 255, 0)));
}

#[test]
fn catppuccin_mocha_selected_row_has_dark_fg() {
    let _lock = TEST_MUTEX.lock().unwrap();
    init_with_mode(2);
    set_theme(ThemeDef::catppuccin_mocha());
    let style = selected_row();
    // Should use Mocha Base (#1E1E2E) not White
    assert_eq!(style.fg, Some(Color::Rgb(30, 30, 46)));
    assert_eq!(style.bg, Some(Color::Rgb(137, 180, 250)));
    COLOR_MODE.store(1, Ordering::Release);
    set_theme(ThemeDef::purple());
}

#[test]
fn catppuccin_latte_footer_key_has_dark_fg() {
    let _lock = TEST_MUTEX.lock().unwrap();
    init_with_mode(2);
    set_theme(ThemeDef::catppuccin_latte());
    let style = footer_key();
    // Should use Latte Text (#4C4F69) not White
    assert_eq!(style.fg, Some(Color::Rgb(76, 79, 105)));
    COLOR_MODE.store(1, Ordering::Release);
    set_theme(ThemeDef::purple());
}

#[test]
fn gruvbox_accent_warning_ansi16_differ() {
    let t = ThemeDef::gruvbox_dark();
    // Accent should no longer collide with warning in ANSI 16
    assert_ne!(t.accent.ansi16, t.warning.ansi16);
}

// --- NO_COLOR override test ---

#[test]
fn no_color_mode_forces_no_color_theme() {
    let _lock = TEST_MUTEX.lock().unwrap();
    COLOR_MODE.store(0, Ordering::Release);
    set_theme(ThemeDef::no_color());

    let style = error();
    assert_eq!(style.fg, None);
    assert!(style.add_modifier.contains(Modifier::BOLD));

    let style = success();
    assert_eq!(style.fg, None);

    let style = warning();
    assert_eq!(style.fg, None);

    let style = border();
    assert_eq!(style.fg, None);

    // Cleanup
    COLOR_MODE.store(1, Ordering::Release);
    set_theme(ThemeDef::purple());
}
