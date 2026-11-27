//! UPnP errors returned to senders as SOAP faults.

use thiserror::Error;

/// Every error a control action can return. The code and description are
/// derived from the variant, so they are defined in exactly one place.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum UpnpError {
    #[error("Invalid Action")]
    InvalidAction,
    #[error("Invalid Args")]
    InvalidArgs,
    #[error("Action Failed")]
    ActionFailed,
    #[error("Transition not available")]
    TransitionNotAvailable,
    #[error("Seek mode not supported")]
    SeekModeNotSupported,
    #[error("Illegal seek target")]
    IllegalSeekTarget,
    #[error("Illegal MIME-type")]
    IllegalMimeType,
    #[error("Invalid InstanceID")]
    InvalidInstanceId,
}

impl UpnpError {
    pub fn code(&self) -> u16 {
        match self {
            UpnpError::InvalidAction => 401,
            UpnpError::InvalidArgs => 402,
            UpnpError::ActionFailed => 501,
            UpnpError::TransitionNotAvailable => 701,
            UpnpError::SeekModeNotSupported => 710,
            UpnpError::IllegalSeekTarget => 711,
            UpnpError::IllegalMimeType => 714,
            UpnpError::InvalidInstanceId => 718,
        }
    }
}
