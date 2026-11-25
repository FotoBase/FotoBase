//! Device pairing ticket: what the pairing QR code carries.
//!
//! A ticket is the host's iroh address (endpoint id, relay URL, direct
//! addresses) plus a one-time secret. Knowing the endpoint id alone is not
//! enough to pair; the secret proves the joiner saw the QR code.

use iroh::EndpointAddr;
use iroh_tickets::{endpoint::EndpointTicket, ParseError, Ticket};
use serde::{Deserialize, Serialize};

use crate::SyncError;

/// Length of the one-time pairing secret.
pub(crate) const TOKEN_LEN: usize = 16;

pub(crate) type Token = [u8; TOKEN_LEN];

/// Everything a second device needs to pair with the host that showed it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PairingTicket {
    addr: EndpointAddr,
    token: Token,
}

#[derive(Serialize, Deserialize)]
enum Wire {
    V1 { addr: Vec<u8>, token: Token },
}

impl PairingTicket {
    pub fn new(addr: EndpointAddr, token: Token) -> Self {
        Self { addr, token }
    }

    /// Where to reach the host.
    pub fn endpoint_addr(&self) -> &EndpointAddr {
        &self.addr
    }

    pub(crate) fn token(&self) -> Token {
        self.token
    }

    /// The text a QR code encodes. Upper case, so QR alphanumeric mode
    /// keeps the code small; parsing accepts any case.
    pub fn to_qr_payload(&self) -> String {
        self.encode_string().to_ascii_uppercase()
    }

    /// Parse the text read from a scanned QR code.
    pub fn from_qr_payload(payload: &str) -> Result<Self, SyncError> {
        let normalized = payload.trim().to_ascii_lowercase();
        Self::decode_string(&normalized).map_err(|e| SyncError::Ticket(e.to_string()))
    }

    /// The QR code as an SVG document, for the pairing dialog.
    pub fn to_qr_svg(&self) -> Result<String, SyncError> {
        let code = qrcode::QrCode::new(self.to_qr_payload().as_bytes())
            .map_err(|e| SyncError::Ticket(e.to_string()))?;
        Ok(code.render::<qrcode::render::svg::Color>().min_dimensions(256, 256).build())
    }
}

impl Ticket for PairingTicket {
    const KIND: &'static str = "fbpair";

    fn encode_bytes(&self) -> Vec<u8> {
        let wire = Wire::V1 {
            addr: EndpointTicket::new(self.addr.clone()).encode_bytes(),
            token: self.token,
        };
        postcard::to_stdvec(&wire).expect("postcard serialization failed")
    }

    fn decode_bytes(bytes: &[u8]) -> Result<Self, ParseError> {
        let Wire::V1 { addr, token } = postcard::from_bytes(bytes)?;
        let addr = EndpointTicket::decode_bytes(&addr)?.into();
        Ok(Self { addr, token })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ticket() -> PairingTicket {
        let id = iroh::SecretKey::from_bytes(&[9; 32]).public();
        PairingTicket::new(EndpointAddr::new(id), [3; TOKEN_LEN])
    }

    #[test]
    fn payload_round_trips_in_any_case() {
        let t = ticket();
        let payload = t.to_qr_payload();
        assert_eq!(payload, payload.to_ascii_uppercase());
        assert_eq!(PairingTicket::from_qr_payload(&payload).unwrap(), t);
        assert_eq!(
            PairingTicket::from_qr_payload(&format!(" {} \n", payload.to_lowercase())).unwrap(),
            t
        );
    }

    #[test]
    fn truncated_payload_is_rejected() {
        let payload = ticket().to_qr_payload();
        assert!(PairingTicket::from_qr_payload(&payload[..payload.len() - 8]).is_err());
    }
}
