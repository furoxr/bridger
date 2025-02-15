use lifeline::dyn_bus::DynBus;
use lifeline::{Bus, Lifeline, Receiver, Sender, Service, Task};
use postage::broadcast;

use bridge_traits::bridge::component::BridgeComponent;
use bridge_traits::bridge::config::Config;
use bridge_traits::bridge::service::BridgeService;
use bridge_traits::bridge::task::BridgeSand;
use component_state::state::BridgeState;
use component_thegraph_liketh::TheGraphLikeEthComponent;
use support_tracker::Tracker;

use crate::bus::DarwiniaEthereumBus;
use crate::config::TaskConfig;
use crate::message::{ToExtrinsicsMessage, ToRedeemMessage};
use crate::service::redeem::handler::RedeemHandler;
use crate::task::DarwiniaEthereumTask;

mod handler;

#[derive(Debug)]
pub struct RedeemService {
    _greet_scan: Lifeline,
    _greet_command: Lifeline,
}

impl BridgeService for RedeemService {}

impl Service for RedeemService {
    type Bus = DarwiniaEthereumBus;
    type Lifeline = anyhow::Result<Self>;

    #[allow(irrefutable_let_patterns)]
    fn spawn(bus: &Self::Bus) -> Self::Lifeline {
        // Datastore
        let state = bus.storage().clone_resource::<BridgeState>()?;
        let microkv = state.microkv_with_namespace(DarwiniaEthereumTask::NAME);
        let tracker = Tracker::new(microkv, "scan.ethereum.redeem");

        // Receiver & Sender
        let mut rx = bus.rx::<ToRedeemMessage>()?;
        let sender_to_redeem_scan = bus.tx::<ToRedeemMessage>()?;
        let mut sender_to_redeem_command = bus.tx::<ToRedeemMessage>()?;
        let sender_to_extrinsics_command = bus.tx::<ToExtrinsicsMessage>()?;
        let sender_to_extrinsics_scan = bus.tx::<ToExtrinsicsMessage>()?;

        let _greet_scan = Self::try_task(
            &format!("{}-service-redeem-scan", DarwiniaEthereumTask::NAME),
            async move {
                start_scan(
                    tracker.clone(),
                    sender_to_extrinsics_scan.clone(),
                    sender_to_redeem_scan.clone(),
                )
                .await;
                Ok(())
            },
        );

        let _greet_command = Self::try_task(
            &format!("{}-service-redeem-command", DarwiniaEthereumTask::NAME),
            async move {
                let mut handler = RedeemHandler::new(
                    sender_to_extrinsics_command.clone(),
                    sender_to_redeem_command.clone(),
                )
                .await;

                while let Some(recv) = rx.recv().await {
                    if let ToRedeemMessage::EthereumTransaction(tx) = recv {
                        if let Err(err) = handler.redeem(tx.clone()).await {
                            log::error!(
                                target: DarwiniaEthereumTask::NAME,
                                "[ethereum] redeem err: {:#?}",
                                err
                            );
                            // TODO: Consider the errors more carefully
                            // Maybe a websocket err, so wait 10 secs to reconnect.
                            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                            handler = RedeemHandler::new(
                                sender_to_extrinsics_command.clone(),
                                sender_to_redeem_command.clone(),
                            )
                            .await;
                            // for any error when handler recreated, we need put this tx back to the
                            // receive queue
                            if let Err(e) = sender_to_redeem_command
                                .send(ToRedeemMessage::EthereumTransaction(tx.clone()))
                                .await
                            {
                                log::error!(
                                    target: DarwiniaEthereumTask::NAME,
                                    "Failed to retry send redeem message, tx: {:?} err: {:?}",
                                    tx,
                                    e
                                );
                            }
                        }
                    }
                }

                Ok(())
            },
        );
        Ok(Self {
            _greet_scan,
            _greet_command,
        })
    }
}

async fn start_scan(
    tracker: Tracker,
    sender_to_extrinsics: broadcast::Sender<ToExtrinsicsMessage>,
    sender_to_redeem: broadcast::Sender<ToRedeemMessage>,
) {
    while let Err(err) = run_scan(
        &tracker,
        sender_to_extrinsics.clone(),
        sender_to_redeem.clone(),
    )
    .await
    {
        log::error!(
            target: DarwiniaEthereumTask::NAME,
            "[ethereum] redeem err {:?}",
            err
        );
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

async fn run_scan(
    tracker: &Tracker,
    sender_to_extrinsics: broadcast::Sender<ToExtrinsicsMessage>,
    sender_to_redeem: broadcast::Sender<ToRedeemMessage>,
) -> anyhow::Result<()> {
    // the graph
    let component_thegraph_liketh = TheGraphLikeEthComponent::restore::<DarwiniaEthereumTask>()?;
    let thegraph_liketh = component_thegraph_liketh.component().await?;

    // task config
    let task_config: TaskConfig = Config::restore_unwrap(DarwiniaEthereumTask::NAME)?;

    let mut handler =
        RedeemHandler::new(sender_to_extrinsics.clone(), sender_to_redeem.clone()).await;
    loop {
        let from = tracker.current().await?;
        let limit = 10usize;

        log::trace!(
            target: DarwiniaEthereumTask::NAME,
            "[ethereum] Track redeem block: {} and limit: {}",
            from,
            limit
        );
        let txs = thegraph_liketh
            .query_transactions(from as u64, limit as u32)
            .await?;
        if txs.is_empty() {
            log::info!(
                target: DarwiniaEthereumTask::NAME,
                "[ethereum] Not found any transactions to redeem"
            );
            tokio::time::sleep(std::time::Duration::from_secs(
                task_config.interval_ethereum,
            ))
            .await;
            continue;
        }
        log::debug!(
            target: DarwiniaEthereumTask::NAME,
            "[ethereum] Found {} transactions wait to redeem",
            txs.len()
        );

        let mut latest_redeem_block_number = None;
        // send transactions to redeem
        for tx in &txs {
            let mut times = 0;
            match handler.redeem(tx.clone()).await {
                Ok(Some(latest)) => {
                    log::trace!(
                        target: DarwiniaEthereumTask::NAME,
                        "[ethereum] Change latest redeemed block number to: {}",
                        latest
                    );
                    latest_redeem_block_number = Some(latest);
                }
                Ok(None) => {
                    log::trace!(
                        target: DarwiniaEthereumTask::NAME,
                        "[ethereum] Latest redeemed block number is: {:?}",
                        latest_redeem_block_number
                    );
                    break;
                }
                Err(e) => {
                    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                    times += 1;
                    handler =
                        RedeemHandler::new(sender_to_extrinsics.clone(), sender_to_redeem.clone())
                            .await;
                    if times > 10 {
                        // todo: write log
                        log::error!(
                            target: DarwiniaEthereumTask::NAME,
                            "[ethereum] Failed to send redeem message. tx: {:?}, err: {:?}",
                            tx,
                            e
                        );
                        break;
                    }
                }
            }
        }

        if latest_redeem_block_number.is_none() {
            log::info!(
                target: DarwiniaEthereumTask::NAME,
                "[ethereum] Not have any block redeemed. please wait affirm"
            );
            tokio::time::sleep(std::time::Duration::from_secs(
                task_config.interval_ethereum,
            ))
            .await;
            continue;
        }

        let latest = latest_redeem_block_number.unwrap();
        log::info!(
            target: DarwiniaEthereumTask::NAME,
            "[ethereum] Set scan redeem block number to: {}",
            latest
        );
        tracker.finish(latest as usize)?;
        tokio::time::sleep(std::time::Duration::from_secs(
            task_config.interval_ethereum,
        ))
        .await;
    }
}
