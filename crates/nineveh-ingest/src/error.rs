use nineveh_core::{ChainId, InvalidChainId, Version};

/// Everything that can go wrong while opening or reading a Transaction Stream.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum IngestError {
    #[error("invalid Transaction Stream endpoint `{endpoint}`")]
    InvalidEndpoint {
        endpoint: String,
        #[source]
        source: tonic::transport::Error,
    },

    #[error("the API key contains characters that can't be sent in a gRPC header")]
    InvalidApiKey,

    #[error("couldn't connect to the Transaction Stream at {endpoint}")]
    Connect {
        endpoint: String,
        #[source]
        source: tonic::transport::Error,
    },

    #[error("the Transaction Stream returned {}", describe_status(.0))]
    Status(Box<tonic::Status>),

    #[error("the stream is for chain {actual}, but chain {expected} was expected")]
    ChainMismatch { expected: ChainId, actual: ChainId },

    #[error("the stream reported an invalid chain id")]
    InvalidChainId(#[from] InvalidChainId),

    #[error("the first response didn't include a chain id, so the chain can't be verified")]
    MissingChainId,

    #[error("version {expected} is missing: the stream jumped to {got}")]
    Gap { expected: Version, got: Version },

    #[error("version {got} arrived out of order: expected {expected} or later")]
    OutOfOrder { expected: Version, got: Version },

    #[error("the processed range {first}..={last} doesn't start at the cursor ({expected})")]
    RangeMismatch {
        expected: Version,
        first: Version,
        last: Version,
    },

    #[error("version {version} is outside the response's processed range {first}..={last}")]
    OutsideRange {
        version: Version,
        first: Version,
        last: Version,
    },

    #[error("the version counter overflowed u64")]
    VersionOverflow,

    /// The shared reader for this network has stopped, so there is nothing to join
    /// (ADR 0021). Retryable: the plane restarts readers, and the project's own
    /// cursor means resuming loses nothing.
    #[error("the shared reader for this network has stopped")]
    ReaderStopped,
}

impl IngestError {
    /// Whether reconnecting and resuming from the committed cursor could succeed.
    ///
    /// Transient network and server conditions are retryable. Anything that means the
    /// data or the configuration is wrong is fatal, because retrying would only repeat it.
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        use tonic::Code;
        match self {
            Self::Connect { .. } | Self::ReaderStopped => true,
            Self::Status(status) => matches!(
                status.code(),
                Code::Unavailable
                    | Code::DeadlineExceeded
                    | Code::ResourceExhausted
                    | Code::Aborted
                    | Code::Internal
                    | Code::Unknown
            ),
            Self::InvalidEndpoint { .. }
            | Self::InvalidApiKey
            | Self::ChainMismatch { .. }
            | Self::InvalidChainId(_)
            | Self::MissingChainId
            | Self::Gap { .. }
            | Self::OutOfOrder { .. }
            | Self::RangeMismatch { .. }
            | Self::OutsideRange { .. }
            | Self::VersionOverflow => false,
        }
    }
}

impl From<tonic::Status> for IngestError {
    fn from(status: tonic::Status) -> Self {
        Self::Status(Box::new(status))
    }
}

/// `Unauthenticated: invalid API key`, or just `Unauthenticated` when the server sent
/// no message.
fn describe_status(status: &tonic::Status) -> String {
    match status.message() {
        "" => format!("{:?}", status.code()),
        message => format!("{:?}: {message}", status.code()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_errors_name_the_code_and_keep_the_server_message() {
        let bare = IngestError::from(tonic::Status::unauthenticated(""));
        assert_eq!(
            bare.to_string(),
            "the Transaction Stream returned Unauthenticated"
        );

        let detailed = IngestError::from(tonic::Status::unavailable("draining"));
        assert_eq!(
            detailed.to_string(),
            "the Transaction Stream returned Unavailable: draining"
        );
        assert!(detailed.is_retryable());
        assert!(!bare.is_retryable());
    }
}
