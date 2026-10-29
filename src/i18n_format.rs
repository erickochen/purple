/// Translate a UI template before inserting user-controlled values.
#[macro_export]
macro_rules! tformat {
    ($key:literal, $($arg:expr),* $(,)?) => {
        $crate::i18n::format_text($key, &[$(format!("{}", $arg)),*])
    };
}
