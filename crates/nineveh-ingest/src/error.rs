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

    /// The shared reader for this network gave up, and the error it gave up on
    /// (ADR 0021).
    ///
    /// Not retryable, and what the reader stopped on is what makes it so: the reader
    /// only stops on an error retrying cannot fix, so it says so once and every
    /// project on the network halts carrying the same reason. The alternative is each
    /// project reopening a stream that will be refused again, which is how a missing
    /// devnet key presented itself as a warning every two seconds and no diagnosis.
    #[error("the shared reader for this network stopped: {reason}")]
    ReaderStopped { reason: String },
}

/// Whether an upstream message says the key's organization is out of monthly credit.
///
/// Geomi bills a Transaction Stream by bytes streamed and minutes held open, and caps
/// what an organization may spend each month. Past the cap every call is refused with
/// *"Blocked due to `MonthlyCredit` cap. Your organization has used up its monthly
/// credit."* — a 429 over REST, `ResourceExhausted` over gRPC, which are the same
/// codes a transient rate limit uses.
///
/// They have to be told apart, because this one does not clear. The credit refreshes
/// at the start of the next month, so a pipeline that treats it as transient retries
/// every 30 seconds for days while Studio says *Retrying* — the same failure the
/// [`IngestError::ReaderStopped`] note describes, arriving by a different door. The
/// message is the only thing that distinguishes them.
pub(crate) fn is_credit_cap(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("monthlycredit") || message.contains("monthly credit")
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
            Self::Connect { .. } => true,
            // The credit cap arrives as `ResourceExhausted`, and retrying it is a
            // project spinning until the month turns over.
            Self::Status(status) => {
                matches!(
                    status.code(),
                    Code::Unavailable
                        | Code::DeadlineExceeded
                        | Code::ResourceExhausted
                        | Code::Aborted
                        | Code::Internal
                        | Code::Unknown
                ) && !is_credit_cap(status.message())
            }
            Self::InvalidEndpoint { .. }
            | Self::InvalidApiKey
            | Self::ChainMismatch { .. }
            | Self::InvalidChainId(_)
            | Self::MissingChainId
            | Self::Gap { .. }
            | Self::OutOfOrder { .. }
            | Self::RangeMismatch { .. }
            | Self::OutsideRange { .. }
            | Self::VersionOverflow
            | Self::ReaderStopped { .. } => false,
        }
    }

    /// Whether this is the organization's monthly credit cap, which is a billing
    /// problem rather than a broken project: see [`is_credit_cap`].
    #[must_use]
    pub fn is_credit_cap(&self) -> bool {
        match self {
            Self::Status(status) => is_credit_cap(status.message()),
            // A reader that stopped carries the reason it stopped on, which may be
            // this one (ADR 0021); every project on the network gets it.
            Self::ReaderStopped { reason } => is_credit_cap(reason),
            _ => false,
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

    /// The same cap over gRPC, where it arrives as `ResourceExhausted` — the code a
    /// transient throttle also uses.
    #[test]
    fn the_monthly_credit_cap_is_fatal_though_it_is_resource_exhausted() {
        let capped = IngestError::from(tonic::Status::resource_exhausted(
            "Blocked due to MonthlyCredit cap. Your organization has used up its \
             monthly credit.",
        ));
        assert!(!capped.is_retryable());
        assert!(capped.is_credit_cap());

        let throttled = IngestError::from(tonic::Status::resource_exhausted("too many streams"));
        assert!(throttled.is_retryable());
        assert!(!throttled.is_credit_cap());

        // A shared reader hands every project on the network the reason it stopped.
        let relayed = IngestError::ReaderStopped {
            reason: "the Transaction Stream returned ResourceExhausted: Blocked due to \
                     MonthlyCredit cap"
                .into(),
        };
        assert!(!relayed.is_retryable());
        assert!(relayed.is_credit_cap());
    }
}
