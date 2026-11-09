//! Shared fail-closed shell pin-query classification.
use crate::report::CandidateDisposition;

const ERROR_NOT_FOUND_HRESULT: i32 = 0x8007_0490_u32 as i32;

fn is_missing_app_id(error: &winvd::Error) -> bool {
    matches!(error, winvd::Error::ComError(hresult) if hresult.0 == ERROR_NOT_FOUND_HRESULT)
}

pub fn disposition(
    window: &Result<bool, winvd::Error>,
    app: &Result<bool, winvd::Error>,
) -> CandidateDisposition {
    match (window, app) {
        (Ok(_), Ok(_)) => CandidateDisposition::Observed,
        (Ok(_), Err(error)) if is_missing_app_id(error) => CandidateDisposition::Observed,
        (Err(winvd::Error::WindowNotFound), Err(winvd::Error::WindowNotFound)) => {
            CandidateDisposition::Unsupported
        }
        _ => CandidateDisposition::QueryFailed,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::core::HRESULT;

    #[test]
    fn only_matching_missing_views_are_unsupported() {
        use CandidateDisposition::*;
        use winvd::Error::{ComError, WindowNotFound};
        let missing_app_id = ComError(HRESULT(ERROR_NOT_FOUND_HRESULT));
        let other_com_error = ComError(HRESULT(0x8000_4005_u32 as i32));
        assert_eq!(disposition(&Ok(false), &Ok(true)), Observed);
        assert_eq!(disposition(&Ok(false), &Err(missing_app_id)), Observed);
        assert_eq!(
            disposition(&Ok(true), &Err(ComError(HRESULT(ERROR_NOT_FOUND_HRESULT)))),
            Observed
        );
        assert_eq!(
            disposition(&Err(WindowNotFound), &Err(WindowNotFound)),
            Unsupported
        );
        assert_eq!(disposition(&Ok(true), &Err(WindowNotFound)), QueryFailed);
        assert_eq!(disposition(&Err(WindowNotFound), &Ok(false)), QueryFailed);
        assert_eq!(disposition(&Ok(false), &Err(other_com_error)), QueryFailed);
        assert_eq!(
            disposition(
                &Err(WindowNotFound),
                &Err(ComError(HRESULT(ERROR_NOT_FOUND_HRESULT)))
            ),
            QueryFailed
        );
        assert_eq!(
            disposition(
                &Err(winvd::Error::RpcServerNotAvailable),
                &Err(WindowNotFound)
            ),
            QueryFailed
        );
    }
}
