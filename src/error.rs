//! Errori mostrati all'utente. Il testo si sceglie al momento di mostrarlo, nella lingua dell'interfaccia.

use crate::i18n::{Lang, tr};
use std::io;

#[derive(Debug, Clone, PartialEq)]
pub enum Error {
    TmdbKeyMissing,
    TmdbKeyInvalid,
    OsKeyMissing,
    OsKeyInvalid,
    OsAccountMissing,
    OsLoginFailed,
    OsDailyLimit,
    NoResults,
    NothingToSearch,
    NameNotRecognized,
    TargetExists,
    OriginalExists,
    MovedFileGone,
    FileTooSmall,
    Http(u16),
    Network(String),
    BadResponse(String),
    Io(String),
}

impl Error {
    pub fn localized(&self, l: Lang) -> String {
        let key = match self {
            Error::TmdbKeyMissing => "err_tmdb_key_missing",
            Error::TmdbKeyInvalid => "err_tmdb_key_invalid",
            Error::OsKeyMissing => "err_os_key_missing",
            Error::OsKeyInvalid => "err_os_key_invalid",
            Error::OsAccountMissing => "err_os_account_missing",
            Error::OsLoginFailed => "err_os_login",
            Error::OsDailyLimit => "err_os_daily_limit",
            Error::NoResults => "err_no_results",
            Error::NothingToSearch => "err_nothing_to_search",
            Error::NameNotRecognized => "err_name_not_recognized",
            Error::TargetExists => "err_target_exists",
            Error::OriginalExists => "err_original_exists",
            Error::MovedFileGone => "err_moved_file_gone",
            Error::FileTooSmall => "err_file_too_small",
            Error::Http(429) => "err_too_many_requests",
            Error::Http(_) => "err_http",
            Error::Network(_) => "err_network",
            Error::BadResponse(_) => "err_bad_response",
            Error::Io(_) => "err_io",
        };
        match self {
            Error::Http(429) => tr(l, key).into(),
            Error::Http(code) => format!("{} {code}", tr(l, key)),
            Error::Network(d) | Error::BadResponse(d) | Error::Io(d) => {
                format!("{}: {d}", tr(l, key))
            }
            _ => tr(l, key).into(),
        }
    }
}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ogni_errore_ha_un_testo_in_entrambe_le_lingue() {
        let all = [
            Error::TmdbKeyMissing,
            Error::TmdbKeyInvalid,
            Error::OsKeyMissing,
            Error::OsKeyInvalid,
            Error::OsAccountMissing,
            Error::OsLoginFailed,
            Error::OsDailyLimit,
            Error::NoResults,
            Error::NothingToSearch,
            Error::NameNotRecognized,
            Error::TargetExists,
            Error::OriginalExists,
            Error::MovedFileGone,
            Error::FileTooSmall,
            Error::Http(429),
            Error::Http(500),
            Error::Network("x".into()),
            Error::BadResponse("x".into()),
            Error::Io("x".into()),
        ];
        for e in &all {
            for l in Lang::ALL {
                assert!(
                    !e.localized(l).starts_with("err_"),
                    "manca la traduzione di {e:?} in {l:?}"
                );
            }
        }
        assert_eq!(Error::Http(500).localized(Lang::En), "HTTP error 500");
    }
}
