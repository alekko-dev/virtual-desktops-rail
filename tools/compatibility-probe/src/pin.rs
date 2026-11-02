//! Shared fail-closed shell pin-query classification.
use crate::report::CandidateDisposition;

pub fn disposition(
    window: &Result<bool, winvd::Error>,
    app: &Result<bool, winvd::Error>,
) -> CandidateDisposition {
    match (window, app) {
        (Ok(_), Ok(_)) => CandidateDisposition::Observed,
        (Err(winvd::Error::WindowNotFound), Err(winvd::Error::WindowNotFound)) => {
            CandidateDisposition::Unsupported
        }
        _ => CandidateDisposition::QueryFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_matching_missing_views_are_unsupported() {
        use CandidateDisposition::*;
        use winvd::Error::WindowNotFound;
        assert_eq!(disposition(&Ok(false), &Ok(true)), Observed);
        assert_eq!(
            disposition(&Err(WindowNotFound), &Err(WindowNotFound)),
            Unsupported
        );
        assert_eq!(disposition(&Ok(true), &Err(WindowNotFound)), QueryFailed);
        assert_eq!(disposition(&Err(WindowNotFound), &Ok(false)), QueryFailed);
        assert_eq!(
            disposition(
                &Err(winvd::Error::RpcServerNotAvailable),
                &Err(WindowNotFound)
            ),
            QueryFailed
        );
    }
}
