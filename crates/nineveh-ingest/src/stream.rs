use std::ops::RangeInclusive;
use std::time::Duration;

use nineveh_core::{ChainId, Version};
use secrecy::ExposeSecret;
use tonic::metadata::{AsciiMetadataValue, MetadataValue};
use tonic::transport::{ClientTlsConfig, Endpoint};
use tracing::debug;

use crate::contiguity::Contiguity;
use crate::proto::client::raw_data_client::RawDataClient;
use crate::proto::indexer::{GetTransactionsRequest, TransactionsResponse};
use crate::proto::transaction::Transaction;
use crate::{IngestError, StreamConfig};

/// One response from the stream, already checked for chain identity and contiguity.
#[derive(Debug)]
pub struct Batch {
    pub chain_id: ChainId,
    /// Transactions in strictly increasing version order.
    pub transactions: Vec<Transaction>,
    /// The versions the server scanned to produce this batch, when it reports them.
    /// With a filter this can be wider than the delivered transactions.
    pub processed_range: Option<RangeInclusive<Version>>,
}

/// An open Transaction Stream that delivers ordered, gap-checked batches.
#[derive(Debug)]
pub struct TransactionStream {
    responses: tonic::Streaming<TransactionsResponse>,
    contiguity: Contiguity,
    expected_chain_id: Option<ChainId>,
    /// The chain id from the first response. Every later response must match it.
    chain_id: Option<ChainId>,
}

impl TransactionStream {
    /// Connect and start streaming from `config.starting_version`.
    ///
    /// # Errors
    ///
    /// Fails if the endpoint or API key is malformed, the connection can't be made, or
    /// the server rejects the request.
    pub async fn connect(config: StreamConfig) -> Result<Self, IngestError> {
        let channel =
            endpoint(&config)?
                .connect()
                .await
                .map_err(|source| IngestError::Connect {
                    endpoint: config.endpoint.clone(),
                    source,
                })?;

        let authorization = config.api_key.as_ref().map(bearer).transpose()?;
        let mut client =
            RawDataClient::with_interceptor(channel, move |mut req: tonic::Request<()>| {
                if let Some(value) = &authorization {
                    req.metadata_mut().insert("authorization", value.clone());
                }
                Ok(req)
            })
            .max_decoding_message_size(config.max_message_size);
        if let Some(compression) = config.compression {
            client = client.accept_compressed(compression.into());
        }

        let request = GetTransactionsRequest {
            starting_version: Some(config.starting_version.get()),
            transactions_count: config.transactions_count,
            batch_size: config.batch_size,
            transaction_filter: config.filter.clone(),
        };
        debug!(
            endpoint = %config.endpoint,
            starting_version = %config.starting_version,
            filtered = request.transaction_filter.is_some(),
            "opening transaction stream"
        );
        let responses = client.get_transactions(request).await?.into_inner();

        Ok(Self {
            responses,
            contiguity: Contiguity::new(config.starting_version, config.filter.is_some()),
            expected_chain_id: config.expected_chain_id,
            chain_id: None,
        })
    }

    /// The next version the stream will account for. Every earlier version has been
    /// delivered or, under a filter, confirmed as not matching.
    #[must_use]
    pub fn next_version(&self) -> Version {
        self.contiguity.next()
    }

    /// Receive the next batch. Returns `Ok(None)` when the server ends the stream,
    /// which happens after `transactions_count` transactions.
    ///
    /// # Errors
    ///
    /// Fails on a transport or server error, a chain id that doesn't match, or a
    /// response that skips, repeats or reorders versions. After an error the stream
    /// shouldn't be read again. Reconnect from [`Self::next_version`] instead.
    pub async fn next_batch(&mut self) -> Result<Option<Batch>, IngestError> {
        let Some(response) = self.responses.message().await? else {
            return Ok(None);
        };

        let chain_id = self.verify_chain(response.chain_id)?;
        let processed_range = response
            .processed_range
            .map(|r| Version::new(r.first_version)..=Version::new(r.last_version));

        self.contiguity.advance(
            response
                .transactions
                .iter()
                .map(|tx| Version::new(tx.version)),
            processed_range.clone(),
        )?;

        Ok(Some(Batch {
            chain_id,
            transactions: response.transactions,
            processed_range,
        }))
    }

    fn verify_chain(&mut self, reported: Option<u64>) -> Result<ChainId, IngestError> {
        let reported = reported.map(ChainId::try_from).transpose()?;
        match (self.chain_id, reported) {
            (None, None) => Err(IngestError::MissingChainId),
            (Some(known), Some(actual)) if known != actual => Err(IngestError::ChainMismatch {
                expected: known,
                actual,
            }),
            (Some(known), _) => Ok(known),
            (None, Some(first)) => {
                if let Some(expected) = self.expected_chain_id
                    && first != expected
                {
                    return Err(IngestError::ChainMismatch {
                        expected,
                        actual: first,
                    });
                }
                self.chain_id = Some(first);
                Ok(first)
            }
        }
    }
}

fn endpoint(config: &StreamConfig) -> Result<Endpoint, IngestError> {
    let invalid = |source| IngestError::InvalidEndpoint {
        endpoint: config.endpoint.clone(),
        source,
    };

    let mut endpoint = Endpoint::from_shared(config.endpoint.clone())
        .map_err(invalid)?
        .user_agent(concat!("nineveh/", env!("CARGO_PKG_VERSION")))
        .map_err(invalid)?
        .connect_timeout(config.connect_timeout)
        .tcp_nodelay(true)
        .http2_adaptive_window(true)
        .http2_keep_alive_interval(Duration::from_secs(30))
        .keep_alive_timeout(Duration::from_secs(10))
        .keep_alive_while_idle(true);

    if config.endpoint.starts_with("https://") {
        endpoint = endpoint
            .tls_config(ClientTlsConfig::new().with_webpki_roots())
            .map_err(invalid)?;
    }
    Ok(endpoint)
}

fn bearer(api_key: &secrecy::SecretString) -> Result<AsciiMetadataValue, IngestError> {
    let mut value: AsciiMetadataValue =
        MetadataValue::try_from(format!("Bearer {}", api_key.expose_secret()))
            .map_err(|_| IngestError::InvalidApiKey)?;
    value.set_sensitive(true);
    Ok(value)
}
