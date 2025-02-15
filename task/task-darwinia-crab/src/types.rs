use std::fmt::{Display, Formatter};
use std::str::FromStr;

use bp_messages::LaneId;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sp_core::crypto::Pair;

use crate::traits::CliChain;

#[derive(Debug, Clone, Eq, PartialEq, Deserialize, Serialize, strum::EnumString)]
pub enum BridgeName {
    #[strum(serialize = "darwinia-to-crab")]
    DarwiniaToCrab,
    #[strum(serialize = "crab-to-darwinia")]
    CrabToDarwinia,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChainInfo {
    pub host: String,
    pub port: u16,
    pub signer: Option<String>,
    pub secure: bool,
    pub signer_password: Option<String>,
    pub transactions_mortality: Option<u32>,
}

impl ChainInfo {
    /// Convert connection params into Substrate client.
    pub async fn to_substrate_relay_chain<C: CliChain>(
        &self,
    ) -> anyhow::Result<relay_substrate_client::Client<C>> {
        Ok(
            relay_substrate_client::Client::new(relay_substrate_client::ConnectionParams {
                host: self.host.clone(),
                port: self.port,
                secure: self.secure,
            })
            .await,
        )
    }

    /// Parse signing params into chain-specific KeyPair.
    pub fn to_keypair<C: CliChain>(&self) -> anyhow::Result<C::KeyPair> {
        let signer = match self.signer.clone() {
            Some(v) => v,
            None => anyhow::bail!("The chain [{}:{}] not set signer", self.host, self.port,),
        };
        C::KeyPair::from_string(&signer, self.signer_password.as_deref())
            .map_err(|e| anyhow::format_err!("{:?}", e))
    }

    pub fn transactions_mortality(&self) -> anyhow::Result<Option<u32>> {
        self.transactions_mortality
            .map(|transactions_mortality| {
                if !(4..=65536).contains(&transactions_mortality)
                    || !transactions_mortality.is_power_of_two()
                {
                    Err(anyhow::format_err!(
                        "Transactions mortality {} is not a power of two in a [4; 65536] range",
                        transactions_mortality,
                    ))
                } else {
                    Ok(transactions_mortality)
                }
            })
            .transpose()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitBridge {
    pub bridge: BridgeName,
    pub source: ChainInfo,
    pub target: ChainInfo,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HexLaneId(pub LaneId);

impl From<HexLaneId> for LaneId {
    fn from(lane_id: HexLaneId) -> LaneId {
        lane_id.0
    }
}

impl std::str::FromStr for HexLaneId {
    type Err = hex::FromHexError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut lane_id = LaneId::default();
        hex::decode_to_slice(s, &mut lane_id)?;
        Ok(HexLaneId(lane_id))
    }
}

impl Display for HexLaneId {
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        let lane_id = self.0;
        let hex_text = hex::encode(lane_id);
        f.write_str(&hex_text[..])
    }
}

impl<'de> Deserialize<'de> for HexLaneId {
    fn deserialize<D>(deserializer: D) -> Result<Self, <D as Deserializer<'de>>::Error>
    where
        D: Deserializer<'de>,
    {
        let hex_text: String = Deserialize::deserialize(deserializer)?;
        let lane = HexLaneId::from_str(&hex_text[..]).map_err(serde::de::Error::custom)?;
        Ok(lane)
    }
}

impl Serialize for HexLaneId {
    fn serialize<S>(&self, serializer: S) -> Result<<S as Serializer>::Ok, <S as Serializer>::Error>
    where
        S: Serializer,
    {
        let hex_text = self.to_string();
        serializer.serialize_str(&hex_text[..])
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PrometheusParamsInfo {
    /// Do not expose a Prometheus metric endpoint.
    pub no_prometheus: bool,
    /// Expose Prometheus endpoint at given interface.
    #[serde(skip_serializing_if = "String::is_empty")]
    #[serde(default)]
    pub prometheus_host: String,
    /// Expose Prometheus endpoint at given port.
    pub prometheus_port: u16,
}

impl From<PrometheusParamsInfo> for relay_utils::metrics::MetricsParams {
    fn from(cli_params: PrometheusParamsInfo) -> relay_utils::metrics::MetricsParams {
        if !cli_params.no_prometheus {
            Some(relay_utils::metrics::MetricsAddress {
                host: cli_params.prometheus_host,
                port: cli_params.prometheus_port,
            })
            .into()
        } else {
            None.into()
        }
    }
}

#[derive(Debug, Clone)]
pub struct RelayHeadersAndMessagesInfo {
    pub source: ChainInfo,
    pub target: ChainInfo,

    pub lanes: Vec<HexLaneId>,
    pub prometheus_params: PrometheusParamsInfo,
    pub create_relayers_fund_accounts: bool,
    pub only_mandatory_headers: bool,

    pub darwinia_messages_pallet_owner_signing: MessagesPalletOwnerSigningParams,
    pub crab_messages_pallet_owner_signing: MessagesPalletOwnerSigningParams,
}

#[derive(Debug, Clone)]
pub struct MessagesPalletOwnerSigningParams {
    pub messages_pallet_owner: Option<String>,
    pub messages_pallet_owner_password: Option<String>,
}

#[allow(dead_code)]
impl MessagesPalletOwnerSigningParams {
    /// Parse signing params into chain-specific KeyPair.
    pub fn to_keypair<Chain: CliChain>(&self) -> anyhow::Result<Option<Chain::KeyPair>> {
        let messages_pallet_owner = match self.messages_pallet_owner {
            Some(ref messages_pallet_owner) => messages_pallet_owner,
            None => return Ok(None),
        };
        Chain::KeyPair::from_string(
            messages_pallet_owner,
            self.messages_pallet_owner_password.as_deref(),
        )
        .map_err(|e| anyhow::format_err!("{:?}", e))
        .map(Some)
    }
}
