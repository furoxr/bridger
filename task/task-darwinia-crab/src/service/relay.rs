use darwinia_common_primitives::AccountId;
use futures::{FutureExt, TryFutureExt};
use lifeline::{Bus, Lifeline, Receiver, Service, Task};
use relay_substrate_client::{AccountIdOf, Chain, Client, TransactionSignScheme};
use relay_utils::metrics::MetricsParams;
use sp_core::Pair;
use substrate_relay_helper::messages_lane::{MessagesRelayParams, SubstrateMessageLane};
use substrate_relay_helper::on_demand_headers::OnDemandHeadersRelay;

use bridge_traits::bridge::config::Config;
use bridge_traits::bridge::service::BridgeService;
use bridge_traits::bridge::task::BridgeSand;
use component_crab_s2s::{CrabChain, CrabRelayStrategy};
use component_darwinia_s2s::{DarwiniaChain, DarwiniaRelayStrategy};

use crate::bus::DarwiniaCrabBus;
use crate::chains::crab::{
    CrabFinalityToDarwinia, CrabMessagesToDarwinia, CrabMessagesToDarwiniaRunner,
};
use crate::chains::darwinia::{
    DarwiniaFinalityToCrab, DarwiniaMessagesToPangoro, DarwiniaMessagesToPangoroRunner,
};
use crate::config::{ChainInfoConfig, RelayConfig};
use crate::message::DarwiniaCrabMessageSend;
use crate::task::DarwiniaCrabTask;
use crate::types::{MessagesPalletOwnerSigningParams, RelayHeadersAndMessagesInfo};

#[derive(Debug)]
pub struct RelayService {
    _greet: Lifeline,
}

impl BridgeService for RelayService {}

impl Service for RelayService {
    type Bus = DarwiniaCrabBus;
    type Lifeline = anyhow::Result<Self>;

    fn spawn(bus: &Self::Bus) -> Self::Lifeline {
        let mut rx = bus.rx::<DarwiniaCrabMessageSend>()?;
        let config_darwinia: ChainInfoConfig =
            Config::restore_with_namespace_unwrap(DarwiniaCrabTask::NAME, "darwinia")?;
        let config_crab: ChainInfoConfig =
            Config::restore_with_namespace_unwrap(DarwiniaCrabTask::NAME, "crab")?;
        let config_relay: RelayConfig = Config::restore_unwrap(DarwiniaCrabTask::NAME)?;

        let _greet = Self::try_task(&format!("{}-relay", DarwiniaCrabTask::NAME), async move {
            while let Some(message) = rx.recv().await {
                match message {
                    DarwiniaCrabMessageSend::Relay => {}
                    _ => continue,
                }
                let (source_chain, target_chain) = (
                    config_darwinia
                        .to_chain_info_with_expect_signer(config_relay.signer_darwinia.clone())?,
                    config_crab
                        .to_chain_info_with_expect_signer(config_relay.signer_crab.clone())?,
                );

                let relay_info = RelayHeadersAndMessagesInfo {
                    source: source_chain,
                    target: target_chain,
                    lanes: config_relay.lanes.clone(),
                    prometheus_params: config_relay.prometheus_params.clone(),
                    create_relayers_fund_accounts: config_relay.create_relayers_fund_accounts,
                    only_mandatory_headers: config_relay.only_mandatory_headers,
                    darwinia_messages_pallet_owner_signing: MessagesPalletOwnerSigningParams {
                        messages_pallet_owner: config_relay.darwinia_messages_pallet_owner.clone(),
                        messages_pallet_owner_password: config_relay
                            .darwinia_messages_pallet_owner_password
                            .clone(),
                    },
                    crab_messages_pallet_owner_signing: MessagesPalletOwnerSigningParams {
                        messages_pallet_owner: config_relay.crab_messages_pallet_owner.clone(),
                        messages_pallet_owner_password: config_relay
                            .crab_messages_pallet_owner_password
                            .clone(),
                    },
                };

                std::thread::spawn(move || futures::executor::block_on(bridge_relay(relay_info)))
                    .join()
                    .map_err(|_| anyhow::Error::msg("Failed to join thread handle"))??;

                // bridge_relay(relay_info).await?;
            }
            Ok(())
        });
        Ok(Self { _greet })
    }
}

async fn bridge_relay(relay_info: RelayHeadersAndMessagesInfo) -> anyhow::Result<()> {
    let darwinia_chain = relay_info.source;
    let crab_chain = relay_info.target;

    let darwinia_client = darwinia_chain
        .to_substrate_relay_chain::<DarwiniaChain>()
        .await?;
    let crab_client = crab_chain.to_substrate_relay_chain::<CrabChain>().await?;

    let darwinia_sign = darwinia_chain.to_keypair::<DarwiniaChain>()?;
    let pangoro_sign = crab_chain.to_keypair::<CrabChain>()?;
    let darwinia_transactions_mortality = darwinia_chain.transactions_mortality()?;
    let pangoro_transactions_mortality = crab_chain.transactions_mortality()?;

    let lanes = relay_info.lanes;

    let metrics_params: MetricsParams = relay_info.prometheus_params.clone().into();
    let metrics_params = relay_utils::relay_metrics(None, metrics_params).into_params();

    // const METRIC_IS_SOME_PROOF: &str = "it is `None` when metric has been already registered; \
    // 			this is the command entrypoint, so nothing has been registered yet; \
    // 			qed";

    if relay_info.create_relayers_fund_accounts {
        let relayer_fund_acount_id = pallet_bridge_messages::relayer_fund_account_id::<
            AccountIdOf<DarwiniaChain>,
            darwinia_bridge_primitives::AccountIdConverter,
        >();
        let relayers_fund_account_balance = darwinia_client
            .free_native_balance(relayer_fund_acount_id.clone())
            .await;
        if let Err(relay_substrate_client::Error::AccountDoesNotExist) =
            relayers_fund_account_balance
        {
            log::info!(target: "bridge", "Going to create relayers fund account at {}.", DarwiniaChain::NAME);
            create_darwinia_account(
                darwinia_client.clone(),
                darwinia_sign.clone(),
                relayer_fund_acount_id,
            )
            .await?;
        }

        let relayer_fund_acount_id = pallet_bridge_messages::relayer_fund_account_id::<
            AccountIdOf<CrabChain>,
            darwinia_bridge_primitives::AccountIdConverter,
        >();
        let relayers_fund_account_balance = crab_client
            .free_native_balance(relayer_fund_acount_id.clone())
            .await;
        if let Err(relay_substrate_client::Error::AccountDoesNotExist) =
            relayers_fund_account_balance
        {
            log::info!(target: "bridge", "Going to create relayers fund account at {}.", CrabChain::NAME);
            create_pangoro_account(
                crab_client.clone(),
                pangoro_sign.clone(),
                relayer_fund_acount_id,
            )
            .await?;
        }
    }

    let darwinia_to_crab_on_demand_headers = OnDemandHeadersRelay::new(
        darwinia_client.clone(),
        crab_client.clone(),
        pangoro_transactions_mortality,
        DarwiniaFinalityToCrab::new(crab_client.clone(), pangoro_sign.clone()),
        darwinia_common_primitives::DARWINIA_BLOCKS_PER_SESSION,
        relay_info.only_mandatory_headers,
    );
    let crab_to_darwinia_on_demand_headers = OnDemandHeadersRelay::new(
        crab_client.clone(),
        darwinia_client.clone(),
        darwinia_transactions_mortality,
        CrabFinalityToDarwinia::new(darwinia_client.clone(), darwinia_sign.clone()),
        darwinia_common_primitives::CRAB_BLOCKS_PER_SESSION,
        relay_info.only_mandatory_headers,
    );

    // Need 2x capacity since we consider both directions for each lane
    let mut message_relays = Vec::with_capacity(lanes.len() * 2);
    for lane in lanes {
        let lane = lane.into();

        let darwinia_to_crab_messages = DarwiniaMessagesToPangoroRunner::run(MessagesRelayParams {
            source_client: darwinia_client.clone(),
            source_sign: darwinia_sign.clone(),
            target_client: crab_client.clone(),
            target_sign: pangoro_sign.clone(),
            source_to_target_headers_relay: Some(darwinia_to_crab_on_demand_headers.clone()),
            target_to_source_headers_relay: Some(crab_to_darwinia_on_demand_headers.clone()),
            lane_id: lane,
            metrics_params: metrics_params.clone().disable().metrics_prefix(
                messages_relay::message_lane_loop::metrics_prefix::<
                    <DarwiniaMessagesToPangoro as SubstrateMessageLane>::MessageLane,
                >(&lane),
            ),
            relay_strategy: DarwiniaRelayStrategy::new(
                darwinia_client.clone(),
                AccountId::from(darwinia_sign.public().0),
            ),
        })
        .map_err(|e| anyhow::format_err!("{}", e))
        .boxed();

        let crab_to_darwinia_messages = CrabMessagesToDarwiniaRunner::run(MessagesRelayParams {
            source_client: crab_client.clone(),
            source_sign: pangoro_sign.clone(),
            target_client: darwinia_client.clone(),
            target_sign: darwinia_sign.clone(),
            source_to_target_headers_relay: Some(crab_to_darwinia_on_demand_headers.clone()),
            target_to_source_headers_relay: Some(darwinia_to_crab_on_demand_headers.clone()),
            lane_id: lane,
            metrics_params: metrics_params.clone().disable().metrics_prefix(
                messages_relay::message_lane_loop::metrics_prefix::<
                    <CrabMessagesToDarwinia as SubstrateMessageLane>::MessageLane,
                >(&lane),
            ),
            relay_strategy: CrabRelayStrategy::new(
                crab_client.clone(),
                AccountId::from(pangoro_sign.public().0),
            ),
        })
        .map_err(|e| anyhow::format_err!("{}", e))
        .boxed();

        message_relays.push(darwinia_to_crab_messages);
        message_relays.push(crab_to_darwinia_messages);
    }

    relay_utils::relay_metrics(None, metrics_params)
        .expose()
        .await
        .map_err(|e| anyhow::format_err!("{}", e))?;

    futures::future::select_all(message_relays).await.0
}

async fn create_darwinia_account(
    _left_client: Client<DarwiniaChain>,
    _left_sign: <DarwiniaChain as TransactionSignScheme>::AccountKeyPair,
    _account_id: AccountIdOf<DarwiniaChain>,
) -> anyhow::Result<()> {
    Err(anyhow::format_err!(
        "Account creation is not supported by this bridge"
    ))
}

async fn create_pangoro_account(
    _left_client: Client<CrabChain>,
    _left_sign: <CrabChain as TransactionSignScheme>::AccountKeyPair,
    _account_id: AccountIdOf<CrabChain>,
) -> anyhow::Result<()> {
    Err(anyhow::format_err!(
        "Account creation is not supported by this bridge"
    ))
}
