use drml_common_primitives::AccountId;
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
use component_pangolin_s2s::{PangolinChain, PangolinRelayStrategy};
use component_pangoro_s2s::{PangoroChain, PangoroRelayStrategy};

use crate::bus::PangolinPangoroBus;
use crate::chains::pangolin::{
    PangolinFinalityToPangoro, PangolinMessagesToPangoro, PangolinMessagesToPangoroRunner,
};
use crate::chains::pangoro::{
    PangoroFinalityToPangolin, PangoroMessagesToPangolin, PangoroMessagesToPangolinRunner,
};
use crate::config::{ChainInfoConfig, RelayConfig};
use crate::message::PangolinPangoroMessageSend;
use crate::task::PangolinPangoroTask;
use crate::types::{MessagesPalletOwnerSigningParams, RelayHeadersAndMessagesInfo};

// /// Maximal allowed conversion rate error ratio (abs(real - stored) / stored) that we allow.
// ///
// /// If it is zero, then transaction will be submitted every time we see difference between
// /// stored and real conversion rates. If it is large enough (e.g. > than 10 percents, which is 0.1),
// /// then rational relayers may stop relaying messages because they were submitted using
// /// lesser conversion rate.
// const CONVERSION_RATE_ALLOWED_DIFFERENCE_RATIO: f64 = 0.05;

#[derive(Debug)]
pub struct RelayService {
    _greet: Lifeline,
}

impl BridgeService for RelayService {}

impl Service for RelayService {
    type Bus = PangolinPangoroBus;
    type Lifeline = anyhow::Result<Self>;

    fn spawn(bus: &Self::Bus) -> Self::Lifeline {
        let mut rx = bus.rx::<PangolinPangoroMessageSend>()?;
        let config_pangolin: ChainInfoConfig =
            Config::restore_with_namespace_unwrap(PangolinPangoroTask::NAME, "pangolin")?;
        let config_pangoro: ChainInfoConfig =
            Config::restore_with_namespace_unwrap(PangolinPangoroTask::NAME, "pangoro")?;
        let config_relay: RelayConfig = Config::restore_unwrap(PangolinPangoroTask::NAME)?;

        let _greet = Self::try_task(
            &format!("{}-relay", PangolinPangoroTask::NAME),
            async move {
                while let Some(message) = rx.recv().await {
                    match message {
                        PangolinPangoroMessageSend::Relay => {}
                        _ => continue,
                    }
                    let (source_chain, target_chain) = (
                        config_pangolin.to_chain_info_with_expect_signer(
                            config_relay.signer_pangolin.clone(),
                        )?,
                        config_pangoro.to_chain_info_with_expect_signer(
                            config_relay.signer_pangoro.clone(),
                        )?,
                    );

                    let relay_info = RelayHeadersAndMessagesInfo {
                        source: source_chain,
                        target: target_chain,
                        lanes: config_relay.lanes.clone(),
                        prometheus_params: config_relay.prometheus_params.clone(),
                        create_relayers_fund_accounts: config_relay.create_relayers_fund_accounts,
                        only_mandatory_headers: config_relay.only_mandatory_headers,
                        pangolin_messages_pallet_owner_signing: MessagesPalletOwnerSigningParams {
                            messages_pallet_owner: config_relay
                                .pangolin_messages_pallet_owner
                                .clone(),
                            messages_pallet_owner_password: config_relay
                                .pangolin_messages_pallet_owner_password
                                .clone(),
                        },
                        pangoro_messages_pallet_owner_signing: MessagesPalletOwnerSigningParams {
                            messages_pallet_owner: config_relay
                                .pangoro_messages_pallet_owner
                                .clone(),
                            messages_pallet_owner_password: config_relay
                                .pangoro_messages_pallet_owner_password
                                .clone(),
                        },
                    };

                    std::thread::spawn(move || {
                        futures::executor::block_on(bridge_relay(relay_info))
                    })
                    .join()
                    .map_err(|_| anyhow::Error::msg("Failed to join thread handle"))??;

                    // bridge_relay(relay_info).await?;
                }
                Ok(())
            },
        );
        Ok(Self { _greet })
    }
}

async fn bridge_relay(relay_info: RelayHeadersAndMessagesInfo) -> anyhow::Result<()> {
    let pangolin_chain = relay_info.source;
    let pangoro_chain = relay_info.target;

    let pangolin_client = pangolin_chain
        .to_substrate_relay_chain::<PangolinChain>()
        .await?;
    let pangoro_client = pangoro_chain
        .to_substrate_relay_chain::<PangoroChain>()
        .await?;

    let pangolin_sign = pangolin_chain.to_keypair::<PangolinChain>()?;
    let pangoro_sign = pangoro_chain.to_keypair::<PangoroChain>()?;
    let pangolin_transactions_mortality = pangolin_chain.transactions_mortality()?;
    let pangoro_transactions_mortality = pangoro_chain.transactions_mortality()?;

    let lanes = relay_info.lanes;

    let metrics_params: MetricsParams = relay_info.prometheus_params.clone().into();
    let metrics_params = relay_utils::relay_metrics(None, metrics_params).into_params();

    // const METRIC_IS_SOME_PROOF: &str = "it is `None` when metric has been already registered; \
    // 			this is the command entrypoint, so nothing has been registered yet; \
    // 			qed";

    if relay_info.create_relayers_fund_accounts {
        let relayer_fund_acount_id = pallet_bridge_messages::relayer_fund_account_id::<
            AccountIdOf<PangolinChain>,
            drml_bridge_primitives::AccountIdConverter,
        >();
        let relayers_fund_account_balance = pangolin_client
            .free_native_balance(relayer_fund_acount_id.clone())
            .await;
        if let Err(relay_substrate_client::Error::AccountDoesNotExist) =
            relayers_fund_account_balance
        {
            log::info!(target: "bridge", "Going to create relayers fund account at {}.", PangolinChain::NAME);
            create_pangolin_account(
                pangolin_client.clone(),
                pangolin_sign.clone(),
                relayer_fund_acount_id,
            )
            .await?;
        }

        let relayer_fund_acount_id = pallet_bridge_messages::relayer_fund_account_id::<
            AccountIdOf<PangoroChain>,
            drml_bridge_primitives::AccountIdConverter,
        >();
        let relayers_fund_account_balance = pangoro_client
            .free_native_balance(relayer_fund_acount_id.clone())
            .await;
        if let Err(relay_substrate_client::Error::AccountDoesNotExist) =
            relayers_fund_account_balance
        {
            log::info!(target: "bridge", "Going to create relayers fund account at {}.", PangoroChain::NAME);
            create_pangoro_account(
                pangoro_client.clone(),
                pangoro_sign.clone(),
                relayer_fund_acount_id,
            )
            .await?;
        }
    }

    let pangolin_to_pangoro_on_demand_headers = OnDemandHeadersRelay::new(
        pangolin_client.clone(),
        pangoro_client.clone(),
        pangoro_transactions_mortality,
        PangolinFinalityToPangoro::new(pangoro_client.clone(), pangoro_sign.clone()),
        drml_common_primitives::PANGOLIN_BLOCKS_PER_SESSION,
        relay_info.only_mandatory_headers,
    );
    let pangoro_to_pangolin_on_demand_headers = OnDemandHeadersRelay::new(
        pangoro_client.clone(),
        pangolin_client.clone(),
        pangolin_transactions_mortality,
        PangoroFinalityToPangolin::new(pangolin_client.clone(), pangolin_sign.clone()),
        drml_common_primitives::PANGORO_BLOCKS_PER_SESSION,
        relay_info.only_mandatory_headers,
    );

    // Need 2x capacity since we consider both directions for each lane
    let mut message_relays = Vec::with_capacity(lanes.len() * 2);
    for lane in lanes {
        let lane = lane.into();

        let pangolin_to_pangoro_messages =
            PangolinMessagesToPangoroRunner::run(MessagesRelayParams {
                source_client: pangolin_client.clone(),
                source_sign: pangolin_sign.clone(),
                target_client: pangoro_client.clone(),
                target_sign: pangoro_sign.clone(),
                source_to_target_headers_relay: Some(pangolin_to_pangoro_on_demand_headers.clone()),
                target_to_source_headers_relay: Some(pangoro_to_pangolin_on_demand_headers.clone()),
                lane_id: lane,
                metrics_params: metrics_params.clone().disable().metrics_prefix(
                    messages_relay::message_lane_loop::metrics_prefix::<
                        <PangolinMessagesToPangoro as SubstrateMessageLane>::MessageLane,
                    >(&lane),
                ),
                relay_strategy: PangolinRelayStrategy::new(
                    pangolin_client.clone(),
                    AccountId::from(pangolin_sign.public().0),
                ),
            })
            .map_err(|e| anyhow::format_err!("{}", e))
            .boxed();

        let pangoro_to_pangolin_messages =
            PangoroMessagesToPangolinRunner::run(MessagesRelayParams {
                source_client: pangoro_client.clone(),
                source_sign: pangoro_sign.clone(),
                target_client: pangolin_client.clone(),
                target_sign: pangolin_sign.clone(),
                source_to_target_headers_relay: Some(pangoro_to_pangolin_on_demand_headers.clone()),
                target_to_source_headers_relay: Some(pangolin_to_pangoro_on_demand_headers.clone()),
                lane_id: lane,
                metrics_params: metrics_params.clone().disable().metrics_prefix(
                    messages_relay::message_lane_loop::metrics_prefix::<
                        <PangoroMessagesToPangolin as SubstrateMessageLane>::MessageLane,
                    >(&lane),
                ),
                relay_strategy: PangoroRelayStrategy::new(
                    pangoro_client.clone(),
                    AccountId::from(pangoro_sign.public().0),
                ),
            })
            .map_err(|e| anyhow::format_err!("{}", e))
            .boxed();

        message_relays.push(pangolin_to_pangoro_messages);
        message_relays.push(pangoro_to_pangolin_messages);
    }

    relay_utils::relay_metrics(None, metrics_params)
        .expose()
        .await
        .map_err(|e| anyhow::format_err!("{}", e))?;

    futures::future::select_all(message_relays).await.0
}

async fn create_pangolin_account(
    _left_client: Client<PangolinChain>,
    _left_sign: <PangolinChain as TransactionSignScheme>::AccountKeyPair,
    _account_id: AccountIdOf<PangolinChain>,
) -> anyhow::Result<()> {
    Err(anyhow::format_err!(
        "Account creation is not supported by this bridge"
    ))
}

async fn create_pangoro_account(
    _left_client: Client<PangoroChain>,
    _left_sign: <PangoroChain as TransactionSignScheme>::AccountKeyPair,
    _account_id: AccountIdOf<PangoroChain>,
) -> anyhow::Result<()> {
    Err(anyhow::format_err!(
        "Account creation is not supported by this bridge"
    ))
}
