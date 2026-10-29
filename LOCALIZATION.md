# Optional UI language

English remains the default. Use `purple --lang zh-CN` for Simplified Chinese for one invocation, or `purple --set-language zh-CN` to persist it. Press uppercase `L` in the host list to switch between English and Chinese. Use `--lang en` or `--set-language en` to return to English.

The bundled catalog is `locales/zh-CN.json`. An optional catalog at the Purple config directory's `locales/zh-CN.json` overrides matching bundled entries. Invalid JSON, unreadable overrides or invalid placeholder counts emit a warning and use the bundled catalog, so English and --set-language remain usable. Keys absent from both catalogs fall back to English. The selected language is saved as `language` in the same config directory, respecting Purple/XDG path overrides.

User aliases, paths, commands, SSH configuration keywords, remote output, and API protocols are not translated. Templates are translated before values are substituted. Main navigation, forms, help, and common notifications are covered; provider-specific diagnostics can remain English.

This is an opt-in localization proposal. CONTRIBUTING.md currently specifies English-only shipped text; accepting a non-English catalog requires a maintainer decision. The proposal keeps source messages, code, comments, and this documentation in English.

The L shortcut changes only the session in demo mode or when --lang was supplied. Otherwise it persists the selection and displays the selected language. MCP protocol errors and audit logs remain English. Detail labels are padded by display columns, including CJK and combining characters.
