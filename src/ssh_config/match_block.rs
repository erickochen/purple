//! OpenSSH `Match` blocks as a read-only view over the parsed config.
//!
//! The parser keeps a `Match` line and the lines under it as inert
//! `GlobalLine` elements, so the writer never touches them. This module reads
//! those runs back and decides per host what the config file alone can answer.

use std::path::{Path, PathBuf};

use super::model::{ConfigElement, HostBlock, SshConfigFile};
use super::parser::strip_inline_comment;
use super::pattern::pattern_list_matches;

/// Length of the `Match` keyword, the prefix `is_match_line` verified.
const MATCH_KEYWORD_LEN: usize = "match".len();
/// Comment prefix `HostBlock::tags` reads; a Match block uses the same spelling.
const TAGS_COMMENT_PREFIX: &str = "# purple:tags ";

/// One criterion on a `Match` line, as written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchCriterion {
    /// Keyword in lowercase: `host`, `originalhost`, `all`, `exec`, ...
    pub keyword: String,
    /// The argument. `None` for `all`, `canonical` and `final`.
    pub argument: Option<String>,
    /// A leading `!` on the keyword flips the result.
    pub negated: bool,
}

/// What the config file says about one criterion for one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    Yes,
    No,
    Unknown,
}

/// What a whole block does for one host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockStatus {
    Applies,
    Undecided,
    Skipped,
}

/// What is known about a host when a `Match` line is reached. `None` means
/// the value depends on something outside the file.
#[derive(Debug, Clone, Copy)]
pub struct EvalContext<'a> {
    pub alias: &'a str,
    pub hostname: Option<&'a str>,
    pub user: Option<&'a str>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CriterionVerdict {
    pub criterion: MatchCriterion,
    pub verdict: Verdict,
}

/// Convenience view of one Match block for the TUI.
#[derive(Debug, Clone, Default)]
pub struct MatchBlockEntry {
    pub criteria: Vec<MatchCriterion>,
    /// Text after the `Match` keyword as written, without an inline comment.
    pub criteria_text: String,
    pub hostname: String,
    pub user: String,
    pub identity_file: String,
    pub proxy_jump: String,
    /// `Some(false)` for `ProxyCommand none`, `Some(true)` for any other value.
    pub proxy_command_reaches: Option<bool>,
    /// Every directive as `(key, value)`, in file order.
    pub directives: Vec<(String, String)>,
    pub tags: Vec<String>,
    pub source_file: Option<PathBuf>,
    /// Position among all Host and Match sections in reading order.
    pub order: usize,
    /// `order` of the Match block whose Include brought this block in. ssh
    /// never reads it while that block does not apply.
    pub parent: Option<usize>,
}

/// A Match block evaluated for one host.
#[derive(Debug, Clone)]
pub struct MatchBlockMatch {
    pub entry: MatchBlockEntry,
    pub verdicts: Vec<CriterionVerdict>,
    pub status: BlockStatus,
    /// True when the block sits above the host's own block in the file.
    pub before_host: bool,
    /// Keys from `FIRST_WINS_KEYS` this block sets that the host's own block
    /// also sets, when the block applies and sits above it. ssh keeps the
    /// block's value.
    pub overrides_own: Vec<&'static str>,
}

/// Single-valued keys where the first value ssh reads wins and purple shows
/// the host's own value. Cumulative keys such as IdentityFile are left out.
pub const FIRST_WINS_KEYS: [&str; 5] = ["HostName", "User", "Port", "ProxyJump", "ProxyCommand"];

impl MatchBlockEntry {
    /// True when the block sets `key`, case-insensitively.
    pub fn sets(&self, key: &str) -> bool {
        self.directives
            .iter()
            .any(|(k, _)| k.eq_ignore_ascii_case(key))
    }
}

/// One Host or Match section, in reading order.
pub enum Section<'a> {
    Host {
        order: usize,
        block: &'a HostBlock,
        source_file: Option<&'a Path>,
    },
    Match {
        order: usize,
        raw_line: &'a str,
        body: Vec<&'a str>,
        source_file: Option<&'a Path>,
        /// `order` of the enclosing Match block when this one comes from an
        /// Include inside it.
        parent: Option<usize>,
    },
}

/// Criteria that never take an argument.
fn is_argumentless(keyword: &str) -> bool {
    matches!(keyword, "all" | "canonical" | "final")
}

/// Split a `Match` line into its criteria text and criteria list. Accepts
/// `Match a b`, `Match=a b`, `Match = a b` and the glued `host=a` form. A
/// double-quoted argument keeps its spaces and a `#` token ends the line.
pub fn parse_match_line(line: &str) -> (String, Vec<MatchCriterion>) {
    let trimmed = line.trim();
    let rest = trimmed.get(MATCH_KEYWORD_LEN..).unwrap_or("").trim_start();
    let rest = rest.strip_prefix('=').unwrap_or(rest).trim();
    let rest = if rest.starts_with('#') {
        ""
    } else {
        strip_inline_comment(rest).trim()
    };
    let tokens = split_tokens(rest);
    let mut criteria = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        let (negated, word) = match tokens[i].strip_prefix('!') {
            Some(w) => (true, w),
            None => (false, tokens[i].as_str()),
        };
        let glued = word
            .split_once('=')
            .filter(|(k, v)| !k.is_empty() && !v.is_empty())
            .filter(|(k, _)| !is_argumentless(&k.to_ascii_lowercase()));
        let (keyword, argument) = match glued {
            Some((k, v)) => (k.to_ascii_lowercase(), Some(v.to_string())),
            None => {
                let keyword = word.to_ascii_lowercase();
                let argument = if is_argumentless(&keyword) {
                    None
                } else {
                    i += 1;
                    tokens.get(i).cloned()
                };
                (keyword, argument)
            }
        };
        criteria.push(MatchCriterion {
            keyword,
            argument,
            negated,
        });
        i += 1;
    }
    (rest.to_string(), criteria)
}

/// Whitespace split that keeps a double-quoted run whole, quotes removed.
fn split_tokens(text: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    for c in text.chars() {
        match c {
            '"' => in_quotes = !in_quotes,
            c if c.is_whitespace() && !in_quotes => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            c => current.push(c),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Expand the `%h` and `%%` tokens HostName accepts, as ssh does before it
/// compares a `Match host` criterion.
pub fn expand_hostname_tokens(hostname: &str, alias: &str) -> String {
    let mut out = String::with_capacity(hostname.len());
    let mut chars = hostname.chars();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('h') => out.push_str(alias),
            Some('%') => out.push('%'),
            Some(other) => {
                out.push('%');
                out.push(other);
            }
            None => out.push('%'),
        }
    }
    out
}

fn yes_no(hit: bool) -> Verdict {
    if hit { Verdict::Yes } else { Verdict::No }
}

fn negate(v: Verdict) -> Verdict {
    match v {
        Verdict::Yes => Verdict::No,
        Verdict::No => Verdict::Yes,
        Verdict::Unknown => Verdict::Unknown,
    }
}

/// Decide each criterion from the file alone. Criteria combine as AND, so one
/// `No` skips the block and one `Unknown` without a `No` leaves it undecided.
/// An unknown keyword is never treated as a match.
pub fn evaluate(
    criteria: &[MatchCriterion],
    ctx: &EvalContext<'_>,
) -> (Vec<CriterionVerdict>, BlockStatus) {
    let mut verdicts = Vec::with_capacity(criteria.len());
    for c in criteria {
        let raw = match (c.keyword.as_str(), c.argument.as_deref()) {
            ("all", _) => Verdict::Yes,
            // ssh lowercases host names before comparing; user names keep their case.
            ("originalhost", Some(list)) => yes_no(pattern_list_matches(list, ctx.alias, true)),
            ("host", Some(list)) => match ctx.hostname {
                Some(h) => yes_no(pattern_list_matches(list, h, true)),
                None => Verdict::Unknown,
            },
            ("user", Some(list)) => match ctx.user {
                Some(u) => yes_no(pattern_list_matches(list, u, false)),
                None => Verdict::Unknown,
            },
            // `final` and `canonical` only hold on a later pass, after every
            // value from the first pass is fixed, so they fall through here.
            _ => Verdict::Unknown,
        };
        let verdict = if c.negated { negate(raw) } else { raw };
        verdicts.push(CriterionVerdict {
            criterion: c.clone(),
            verdict,
        });
    }
    let any_no = verdicts.iter().any(|v| v.verdict == Verdict::No);
    let any_unknown = verdicts.iter().any(|v| v.verdict == Verdict::Unknown);
    let status = if any_no {
        BlockStatus::Skipped
    } else if any_unknown || criteria.is_empty() {
        BlockStatus::Undecided
    } else {
        BlockStatus::Applies
    };
    (verdicts, status)
}

impl MatchBlockEntry {
    /// Build the view from a Match line and the raw lines under it.
    pub fn from_section(
        raw_line: &str,
        body: &[&str],
        order: usize,
        source_file: Option<&Path>,
    ) -> Self {
        let (criteria_text, criteria) = parse_match_line(raw_line);
        let mut entry = MatchBlockEntry {
            criteria,
            criteria_text,
            order,
            source_file: source_file.map(Path::to_path_buf),
            ..Default::default()
        };
        let mut tags_seen = false;
        for line in body {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(tags) = trimmed.strip_prefix(TAGS_COMMENT_PREFIX) {
                if !tags_seen {
                    tags_seen = true;
                    entry.tags = tags
                        .split(',')
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(str::to_string)
                        .collect();
                }
                continue;
            }
            if trimmed.starts_with('#') {
                continue;
            }
            let Some((key, value)) = SshConfigFile::parse_directive(trimmed) else {
                continue;
            };
            match key.to_ascii_lowercase().as_str() {
                "hostname" if entry.hostname.is_empty() => entry.hostname.clone_from(&value),
                "user" if entry.user.is_empty() => entry.user.clone_from(&value),
                "identityfile" if entry.identity_file.is_empty() => {
                    entry.identity_file.clone_from(&value);
                }
                "proxyjump" if entry.proxy_jump.is_empty() => entry.proxy_jump.clone_from(&value),
                "proxycommand" if entry.proxy_command_reaches.is_none() => {
                    let reaches = !value.trim().eq_ignore_ascii_case("none");
                    entry.proxy_command_reaches = Some(reaches);
                }
                _ => {}
            }
            entry.directives.push((key, value));
        }
        entry
    }

    /// Record the enclosing Match block, see `parent`.
    pub fn with_parent(mut self, parent: Option<usize>) -> Self {
        self.parent = parent;
        self
    }

    /// Source label for inherited hints: `Match host *.prod`.
    pub fn display_label(&self) -> String {
        format!("Match {}", self.criteria_text)
    }

    /// One criterion as the user wrote it, for example `!user root` or `all`.
    pub fn criterion_label(c: &MatchCriterion) -> String {
        let bang = if c.negated { "!" } else { "" };
        match &c.argument {
            Some(arg) if arg.contains(char::is_whitespace) => {
                format!("{bang}{} \"{arg}\"", c.keyword)
            }
            Some(arg) => format!("{bang}{} {arg}", c.keyword),
            None => format!("{bang}{}", c.keyword),
        }
    }
}

/// Push the global lines a file starts with, up to its first Host or Match
/// line. ssh reads on past a nested Include, so its lines count too.
fn push_leading_lines<'a>(elements: &'a [ConfigElement], body: &mut Vec<&'a str>) {
    for e in elements {
        match e {
            ConfigElement::GlobalLine(line) if !SshConfigFile::is_match_line(line.trim()) => {
                body.push(line.as_str());
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    push_leading_lines(&file.elements, body);
                }
            }
            _ => return,
        }
    }
}

/// Visit every Host and Match section in reading order, includes expanded in
/// place. `order` counts both kinds so callers can compare positions. A
/// section in a nested include reports the innermost file, as patterns do.
pub fn walk_sections<'a>(
    elements: &'a [ConfigElement],
    source_file: Option<&'a Path>,
    order: &mut usize,
    visit: &mut dyn FnMut(Section<'a>),
) {
    walk_sections_under(elements, source_file, None, order, visit);
}

/// `walk_sections` inside the Include of the Match block at `parent`.
fn walk_sections_under<'a>(
    elements: &'a [ConfigElement],
    source_file: Option<&'a Path>,
    parent: Option<usize>,
    order: &mut usize,
    visit: &mut dyn FnMut(Section<'a>),
) {
    let mut i = 0;
    while i < elements.len() {
        match &elements[i] {
            ConfigElement::HostBlock(block) => {
                visit(Section::Host {
                    order: *order,
                    block,
                    source_file,
                });
                *order += 1;
                i += 1;
            }
            ConfigElement::Include(include) => {
                for file in &include.resolved_files {
                    let path = Some(file.path.as_path());
                    walk_sections_under(&file.elements, path, parent, order, visit);
                }
                i += 1;
            }
            ConfigElement::GlobalLine(line) if SshConfigFile::is_match_line(line.trim()) => {
                // ssh resumes a Match block after an Include inside it, so the
                // body runs on past it. The included sections follow the block.
                let mut body = Vec::new();
                let mut nested = Vec::new();
                let mut j = i + 1;
                loop {
                    match elements.get(j) {
                        Some(ConfigElement::GlobalLine(next))
                            if !SshConfigFile::is_match_line(next.trim()) =>
                        {
                            body.push(next.as_str());
                        }
                        Some(ConfigElement::Include(include)) => {
                            // Lines at the top of an included file, before its
                            // first section, still belong to this block.
                            for file in &include.resolved_files {
                                push_leading_lines(&file.elements, &mut body);
                            }
                            nested.push(include);
                        }
                        _ => break,
                    }
                    j += 1;
                }
                let this = *order;
                visit(Section::Match {
                    order: this,
                    raw_line: line,
                    body,
                    source_file,
                    parent,
                });
                *order += 1;
                for include in nested {
                    for file in &include.resolved_files {
                        let path = Some(file.path.as_path());
                        walk_sections_under(&file.elements, path, Some(this), order, visit);
                    }
                }
                i = j;
            }
            ConfigElement::GlobalLine(_) => i += 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx<'a>(
        alias: &'a str,
        hostname: Option<&'a str>,
        user: Option<&'a str>,
    ) -> EvalContext<'a> {
        EvalContext {
            alias,
            hostname,
            user,
        }
    }

    fn crit(keyword: &str, argument: Option<&str>, negated: bool) -> MatchCriterion {
        MatchCriterion {
            keyword: keyword.to_string(),
            argument: argument.map(str::to_string),
            negated,
        }
    }

    fn status(criteria: &[MatchCriterion], ctx: &EvalContext<'_>) -> BlockStatus {
        evaluate(criteria, ctx).1
    }

    #[test]
    fn parse_match_line_pairs_keywords_with_arguments() {
        let (text, c) = parse_match_line("Match host *.prod user root");
        assert_eq!(text, "host *.prod user root");
        assert_eq!(
            c,
            vec![
                crit("host", Some("*.prod"), false),
                crit("user", Some("root"), false)
            ]
        );
    }

    #[test]
    fn parse_match_line_accepts_equals_and_mixed_case() {
        let (_, c) = parse_match_line("  MATCH=OriginalHost web-*");
        assert_eq!(c, vec![crit("originalhost", Some("web-*"), false)]);
        let (_, c) = parse_match_line("Match = all");
        assert_eq!(c, vec![crit("all", None, false)]);
    }

    #[test]
    fn parse_match_line_splits_glued_keyword_and_argument() {
        let (_, c) = parse_match_line("Match host=*.prod !originalhost=web2 user root");
        assert_eq!(
            c,
            vec![
                crit("host", Some("*.prod"), false),
                crit("originalhost", Some("web2"), true),
                crit("user", Some("root"), false)
            ]
        );
        let (_, c) = parse_match_line("Match originalhost=\"web1\"");
        assert_eq!(c, vec![crit("originalhost", Some("web1"), false)]);
    }

    #[test]
    fn parse_match_line_glued_form_never_splits_argumentless_keywords() {
        let (_, c) = parse_match_line("Match final=x all");
        assert_eq!(c, vec![crit("final=x", Some("all"), false)]);
    }

    #[test]
    fn parse_match_line_keeps_quoted_exec_command_whole() {
        let line = "Match host *.example exec \"nc -z foo.vpn 22\"";
        let (_, c) = parse_match_line(line);
        assert_eq!(c[1], crit("exec", Some("nc -z foo.vpn 22"), false));
    }

    #[test]
    fn parse_match_line_stops_at_inline_comment() {
        let (text, c) = parse_match_line("Match originalhost web1 # user root");
        assert_eq!(text, "originalhost web1");
        assert_eq!(c, vec![crit("originalhost", Some("web1"), false)]);
        let (text, c) = parse_match_line("Match exec \"a # b\" # note");
        assert_eq!(text, "exec \"a # b\"");
        assert_eq!(c, vec![crit("exec", Some("a # b"), false)]);
        let (text, c) = parse_match_line("Match #only a comment");
        assert_eq!(text, "");
        assert!(c.is_empty());
    }

    #[test]
    fn parse_match_line_reads_negation_and_argumentless_keywords() {
        let (_, c) = parse_match_line("Match final !user root canonical");
        assert_eq!(
            c,
            vec![
                crit("final", None, false),
                crit("user", Some("root"), true),
                crit("canonical", None, false)
            ]
        );
    }

    #[test]
    fn parse_match_line_missing_argument_gives_none() {
        let (_, c) = parse_match_line("Match host");
        assert_eq!(c, vec![crit("host", None, false)]);
    }

    #[test]
    fn expand_hostname_tokens_replaces_h_and_percent() {
        assert_eq!(
            expand_hostname_tokens("%h.example.com", "web1"),
            "web1.example.com"
        );
        assert_eq!(expand_hostname_tokens("a%%b", "x"), "a%b");
        assert_eq!(expand_hostname_tokens("plain", "x"), "plain");
        assert_eq!(expand_hostname_tokens("%p%", "x"), "%p%");
    }

    #[test]
    fn evaluate_originalhost_and_all_from_alias() {
        let c = [
            crit("originalhost", Some("web-*"), false),
            crit("all", None, false),
        ];
        let (v, s) = evaluate(&c, &ctx("web-1", Some("web-1"), None));
        assert_eq!(v[0].verdict, Verdict::Yes);
        assert_eq!(v[1].verdict, Verdict::Yes);
        assert_eq!(s, BlockStatus::Applies);
    }

    #[test]
    fn evaluate_host_uses_known_hostname_not_alias() {
        let c = [crit("host", Some("*.example.com"), false)];
        let known = ctx("db-proton", Some("db1.example.com"), None);
        let alias_only = ctx("db-proton", Some("db-proton"), None);
        let uncertain = ctx("db-proton", None, None);
        assert_eq!(status(&c, &known), BlockStatus::Applies);
        assert_eq!(status(&c, &alias_only), BlockStatus::Skipped);
        assert_eq!(status(&c, &uncertain), BlockStatus::Undecided);
    }

    #[test]
    fn evaluate_user_needs_a_user_set_earlier() {
        let c = [crit("user", Some("root"), false)];
        let as_root = ctx("a", Some("a"), Some("root"));
        let as_eric = ctx("a", Some("a"), Some("eric"));
        let unset = ctx("a", Some("a"), None);
        assert_eq!(status(&c, &as_root), BlockStatus::Applies);
        assert_eq!(status(&c, &as_eric), BlockStatus::Skipped);
        assert_eq!(status(&c, &unset), BlockStatus::Undecided);
    }

    #[test]
    fn evaluate_negation_flips_yes_and_no_only() {
        let here = ctx("a", Some("a"), None);
        let hit = [crit("originalhost", Some("a"), true)];
        let miss = [crit("originalhost", Some("b"), true)];
        let unknown = [crit("localuser", Some("x"), true)];
        assert_eq!(status(&hit, &here), BlockStatus::Skipped);
        assert_eq!(status(&miss, &here), BlockStatus::Applies);
        assert_eq!(status(&unknown, &here), BlockStatus::Undecided);
    }

    #[test]
    fn evaluate_unknown_keywords_and_no_criteria_are_undecided() {
        let keywords = [
            "exec",
            "localuser",
            "localnetwork",
            "canonical",
            "final",
            "tagged",
            "command",
            "version",
            "sessiontype",
            "somethingnew",
        ];
        for kw in keywords {
            let c = [crit(kw, Some("x"), false)];
            let (v, s) = evaluate(&c, &ctx("a", Some("a"), Some("u")));
            assert_eq!(v[0].verdict, Verdict::Unknown, "{kw}");
            assert_eq!(s, BlockStatus::Undecided, "{kw}");
        }
        let none = ctx("a", Some("a"), None);
        assert_eq!(status(&[], &none), BlockStatus::Undecided);
    }

    #[test]
    fn evaluate_host_names_ignore_case_and_users_do_not() {
        let mixed = ctx("WEB1", Some("Web.Example.COM"), Some("root"));
        let host = [crit("host", Some("*.example.com"), false)];
        let orig = [crit("originalhost", Some("web*"), false)];
        let user = [crit("user", Some("Root"), false)];
        assert_eq!(status(&host, &mixed), BlockStatus::Applies);
        assert_eq!(status(&orig, &mixed), BlockStatus::Applies);
        assert_eq!(status(&user, &mixed), BlockStatus::Skipped);
    }

    #[test]
    fn evaluate_final_alone_is_undecided() {
        let c = [crit("final", None, false)];
        assert_eq!(
            status(&c, &ctx("a", Some("a"), None)),
            BlockStatus::Undecided
        );
    }

    #[test]
    fn evaluate_missing_argument_is_unknown() {
        let c = [crit("originalhost", None, false)];
        assert_eq!(
            status(&c, &ctx("a", Some("a"), None)),
            BlockStatus::Undecided
        );
    }

    #[test]
    fn evaluate_one_no_skips_even_with_unknowns() {
        let c = [
            crit("exec", Some("true"), false),
            crit("originalhost", Some("web-*"), false),
        ];
        let db = ctx("db-1", Some("db-1"), None);
        assert_eq!(status(&c, &db), BlockStatus::Skipped);
    }

    #[test]
    fn from_section_reads_directives_tags_and_proxy_command() {
        let body = [
            "  User root",
            "  # purple:tags prod, lab",
            "  # purple:tags ignored",
            "  ProxyCommand none",
            "  IdentityFile \"~/.ssh/id prod\"",
            "",
            "  # a comment",
        ];
        let e = MatchBlockEntry::from_section("Match host *.prod", &body, 3, None);
        assert_eq!(e.criteria_text, "host *.prod");
        assert_eq!(e.user, "root");
        assert_eq!(e.identity_file, "~/.ssh/id prod");
        assert_eq!(e.proxy_command_reaches, Some(false));
        assert_eq!(e.tags, vec!["prod", "lab"]);
        assert_eq!(e.directives.len(), 3);
        assert_eq!(e.order, 3);
        assert_eq!(e.display_label(), "Match host *.prod");
    }

    #[test]
    fn from_section_first_value_wins_and_any_proxy_command_reaches() {
        let body = [
            "  User first",
            "  User second",
            "  HostName h1",
            "  ProxyJump gw",
            "  ProxyCommand ssh -W %h:%p gw",
            "  ProxyCommand none",
        ];
        let e = MatchBlockEntry::from_section("Match all", &body, 0, None);
        assert_eq!(e.user, "first");
        assert_eq!(e.hostname, "h1");
        assert_eq!(e.proxy_jump, "gw");
        assert_eq!(e.proxy_command_reaches, Some(true));
        assert_eq!(e.directives.len(), 6);
    }

    #[test]
    fn criterion_label_quotes_arguments_with_spaces() {
        let exec = crit("exec", Some("nc -z x 22"), false);
        assert_eq!(
            MatchBlockEntry::criterion_label(&exec),
            "exec \"nc -z x 22\""
        );
        let user = crit("user", Some("root"), true);
        assert_eq!(MatchBlockEntry::criterion_label(&user), "!user root");
        let all = crit("all", None, false);
        assert_eq!(MatchBlockEntry::criterion_label(&all), "all");
    }

    #[test]
    fn walk_sections_numbers_hosts_and_match_blocks_in_order() {
        let elements = SshConfigFile::parse_content(
            "Host a\n  HostName 1\n\n\
             Match originalhost a\n  User x\n\n\
             Match all\n  Port 2\n\
             Host b\n  HostName 2\n",
        );
        let mut seen = Vec::new();
        let mut order = 0;
        walk_sections(&elements, None, &mut order, &mut |s| match s {
            Section::Host { order, block, .. } => {
                seen.push((order, format!("Host {}", block.host_pattern)));
            }
            Section::Match {
                order,
                raw_line,
                body,
                ..
            } => seen.push((order, format!("{raw_line} +{}", body.len()))),
        });
        assert_eq!(
            seen,
            vec![
                (0, "Host a".to_string()),
                (1, "Match originalhost a +2".to_string()),
                (2, "Match all +1".to_string()),
                (3, "Host b".to_string()),
            ]
        );
        assert_eq!(order, 4);
    }

    #[test]
    fn walk_sections_match_body_runs_on_past_an_include() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inc = dir.path().join("extra");
        std::fs::write(&inc, "Port 7\nHost inner\n  HostName 9\n").expect("write include");
        let main = dir.path().join("config");
        let text = format!(
            "Match all\n  Port 1\n  Include {}\n  User root\nHost after\n",
            inc.display()
        );
        std::fs::write(&main, text).expect("write config");
        let env = crate::runtime::env::Env::for_test(dir.path());
        let cfg = SshConfigFile::parse_with_env(&main, &env).expect("parse");
        let mut seen = Vec::new();
        let mut order = 0;
        walk_sections(&cfg.elements, None, &mut order, &mut |s| match s {
            Section::Host { order, block, .. } => seen.push((order, block.host_pattern.clone())),
            Section::Match { order, body, .. } => seen.push((order, body.join("|"))),
        });
        assert_eq!(
            seen,
            vec![
                (0, "  Port 1|Port 7|  User root".to_string()),
                (1, "inner".to_string()),
                (2, "after".to_string()),
            ]
        );
    }

    #[test]
    fn walk_sections_reports_innermost_include_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let inner = dir.path().join("inner");
        std::fs::write(&inner, "Match all\n  User root\n").expect("write inner");
        let outer = dir.path().join("outer");
        let outer_text = format!("Include {}\nHost o\n", inner.display());
        std::fs::write(&outer, outer_text).expect("write outer");
        let main = dir.path().join("config");
        std::fs::write(&main, format!("Include {}\n", outer.display())).expect("write main");
        let cfg =
            SshConfigFile::parse_with_env(&main, &crate::runtime::env::Env::for_test(dir.path()))
                .expect("parse");
        let mut sources = Vec::new();
        let mut order = 0;
        walk_sections(&cfg.elements, None, &mut order, &mut |s| match s {
            Section::Host { source_file, .. } | Section::Match { source_file, .. } => {
                sources.push(source_file.map(Path::to_path_buf));
            }
        });
        assert_eq!(sources, vec![Some(inner), Some(outer)]);
    }
}
