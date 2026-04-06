use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolymarketGatewayErrorKind {
    Auth,
    Connectivity,
    UpstreamResponse,
    Protocol,
    Policy,
    Relayer,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PolymarketGatewayError {
    pub kind: PolymarketGatewayErrorKind,
    pub message: String,
}

impl PolymarketGatewayError {
    pub fn new(kind: PolymarketGatewayErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }

    pub fn auth(message: impl Into<String>) -> Self {
        Self::new(PolymarketGatewayErrorKind::Auth, message)
    }

    pub fn connectivity(message: impl Into<String>) -> Self {
        Self::new(PolymarketGatewayErrorKind::Connectivity, message)
    }

    pub fn upstream_response(message: impl Into<String>) -> Self {
        Self::new(PolymarketGatewayErrorKind::UpstreamResponse, message)
    }

    pub fn protocol(message: impl Into<String>) -> Self {
        Self::new(PolymarketGatewayErrorKind::Protocol, message)
    }

    pub fn policy(message: impl Into<String>) -> Self {
        Self::new(PolymarketGatewayErrorKind::Policy, message)
    }

    pub fn relayer(message: impl Into<String>) -> Self {
        Self::new(PolymarketGatewayErrorKind::Relayer, message)
    }
}

impl fmt::Display for PolymarketGatewayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.kind, self.message)
    }
}

impl std::error::Error for PolymarketGatewayError {}
