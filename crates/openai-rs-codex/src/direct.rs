use crate::Error;

/// Placeholder for the explicitly experimental direct Codex transport.
///
/// Direct OAuth and private endpoint emulation are deliberately not present in
/// the MVP. Enabling the feature makes the boundary visible but construction
/// still fails closed.
#[derive(Debug, Clone, Copy, Default)]
pub struct DirectCodexResponsesClient {
    _private: (),
}

impl DirectCodexResponsesClient {
    pub fn new() -> Result<Self, Error> {
        Err(Error::UnsupportedDirectTransport)
    }
}

#[cfg(test)]
mod tests {
    use super::DirectCodexResponsesClient;
    use crate::Error;

    #[test]
    fn direct_transport_fails_closed() {
        assert!(matches!(
            DirectCodexResponsesClient::new(),
            Err(Error::UnsupportedDirectTransport)
        ));
    }
}
