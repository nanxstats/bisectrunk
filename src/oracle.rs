use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum Classification {
    Good,
    Bad,
    Skip,
    Abort,
}

impl std::fmt::Display for Classification {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let value = match self {
            Self::Good => "good",
            Self::Bad => "bad",
            Self::Skip => "skip",
            Self::Abort => "abort",
        };
        formatter.write_str(value)
    }
}

pub(crate) fn classify_exit(code: i32, timed_out: bool) -> Classification {
    if timed_out || code == 125 {
        Classification::Skip
    } else if code == 0 {
        Classification::Good
    } else if code >= 128 {
        Classification::Abort
    } else {
        Classification::Bad
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exit_protocol_matches_git_bisect_run() {
        assert_eq!(classify_exit(0, false), Classification::Good);
        assert_eq!(classify_exit(1, false), Classification::Bad);
        assert_eq!(classify_exit(125, false), Classification::Skip);
        assert_eq!(classify_exit(127, false), Classification::Bad);
        assert_eq!(classify_exit(128, false), Classification::Abort);
        assert_eq!(classify_exit(0, true), Classification::Skip);
    }
}
