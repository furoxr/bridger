//! Ethereum EthereumRelayHeaderParcel
use std::convert::{TryFrom, TryInto};
use std::fmt::Formatter;

use codec::{Decode, Encode};
use serde::{Deserialize, Serialize};
use sp_core::bytes::to_hex;

use bridge_primitives::error::{BridgeBasicError, BridgeBasicResult};
use bridge_primitives::{bytes, hex};

use crate::block::{EthereumHeader, EthereumHeaderJson};

/// Ethereum EthereumRelayHeaderParcel
#[derive(Encode, Decode, Debug, Default, PartialEq, Eq, Clone, Serialize, Deserialize)]
pub struct EthereumRelayHeaderParcel {
    /// Ethereum header
    pub header: EthereumHeader,
    /// MMR root
    pub mmr_root: [u8; 32],
}

impl EthereumRelayHeaderParcel {
    /// Is same as another parcel
    pub fn is_same_as(&self, another: &EthereumRelayHeaderParcel) -> bool {
        self.header.hash == another.header.hash && self.mmr_root == another.mmr_root
    }
}

impl std::fmt::Display for EthereumRelayHeaderParcel {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        let header = &self.header.to_string();
        let msg = format!(
            "{}\n{:>19}{}",
            header,
            "mmr_root: ",
            to_hex(&self.mmr_root, false)
        );
        write!(f, "{}", msg)
    }
}

/// Ethereum EthereumRelayHeaderParcel JSON
#[derive(Default, Deserialize, Serialize)]
pub struct EthereumRelayHeaderParcelJson {
    /// Ethereum header
    pub header: EthereumHeaderJson,
    /// MMR root
    pub mmr_root: String,
}

impl TryFrom<EthereumRelayHeaderParcelJson> for EthereumRelayHeaderParcel {
    type Error = BridgeBasicError;

    fn try_from(that: EthereumRelayHeaderParcelJson) -> BridgeBasicResult<Self> {
        Ok(EthereumRelayHeaderParcel {
            header: that.header.try_into()?,
            mmr_root: bytes!(that.mmr_root.as_str(), 32),
        })
    }
}

impl TryFrom<EthereumRelayHeaderParcel> for EthereumRelayHeaderParcelJson {
    type Error = BridgeBasicError;
    fn try_from(that: EthereumRelayHeaderParcel) -> BridgeBasicResult<Self> {
        Ok(EthereumRelayHeaderParcelJson {
            header: that.header.try_into()?,
            mmr_root: hex!(&that.mmr_root),
        })
    }
}
