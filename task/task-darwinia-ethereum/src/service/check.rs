use std::time::SystemTime;

use bridge_traits::bridge::component::BridgeComponent;
use bridge_traits::bridge::config::Config;
use lifeline::dyn_bus::DynBus;
use lifeline::{Lifeline, Service, Task};

use bridge_traits::bridge::service::BridgeService;
use bridge_traits::bridge::task::BridgeSand;
use component_darwinia_subxt::component::DarwiniaSubxtComponent;
use component_state::state::BridgeState;
use component_thegraph_liketh::TheGraphLikeEthComponent;
use support_tracker::Tracker;

use crate::bus::DarwiniaEthereumBus;
use crate::config::TaskConfig;
use crate::helpers;
use crate::task::DarwiniaEthereumTask;

/// Check service
#[derive(Debug)]
pub struct CheckService {
    _greet: Lifeline,
}

impl BridgeService for CheckService {}

impl Service for CheckService {
    type Bus = DarwiniaEthereumBus;
    type Lifeline = anyhow::Result<Self>;

    fn spawn(bus: &Self::Bus) -> Self::Lifeline {
        // Datastore
        let state = bus.storage().clone_resource::<BridgeState>()?;
        let microkv = state.microkv_with_namespace(DarwiniaEthereumTask::NAME);
        let tracker = Tracker::new(microkv, "scan.ethereum.check");

        // scan task
        let _greet = Self::try_task(
            &format!("{}-service-check", DarwiniaEthereumTask::NAME),
            async move {
                start(tracker.clone()).await;
                Ok(())
            },
        );
        Ok(Self { _greet })
    }
}

async fn start(tracker: Tracker) {
    while let Err(err) = run(&tracker).await {
        log::error!(
            target: DarwiniaEthereumTask::NAME,
            "ethereum check err {:#?}",
            err
        );
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    }
}

async fn run(tracker: &Tracker) -> anyhow::Result<()> {
    log::info!(
        target: DarwiniaEthereumTask::NAME,
        "ETHEREUM CHECK SERVICE RESTARTING..."
    );

    let component_thegraph_liketh = TheGraphLikeEthComponent::restore::<DarwiniaEthereumTask>()?;
    let thegraph_liketh = component_thegraph_liketh.component().await?;
    let task_config: TaskConfig = Config::restore_unwrap(DarwiniaEthereumTask::NAME)?;

    let component_darwinia_subxt = DarwiniaSubxtComponent::restore::<DarwiniaEthereumTask>()?;
    // Darwinia client
    let darwinia = component_darwinia_subxt.component().await?;

    let mut timing = SystemTime::now();
    loop {
        let from = tracker.current().await?;
        let limit = 1usize;

        log::trace!(
            target: DarwiniaEthereumTask::NAME,
            "[ethereum] Track check block: {} and limit: {}",
            from,
            limit
        );
        let txs = thegraph_liketh
            .query_transactions(from as u64, limit as u32)
            .await?;
        if txs.is_empty() {
            log::info!(
                target: DarwiniaEthereumTask::NAME,
                "[ethereum] All transactions checked"
            );
            tokio::time::sleep(std::time::Duration::from_secs(task_config.interval_check)).await;
            continue;
        }

        let tx = txs.get(0).unwrap();

        let verified = match helpers::is_verified(&darwinia, tx).await {
            Ok(v) => v,
            Err(e) => {
                if let Some(substrate_subxt::Error::Rpc(_)) = e.downcast_ref::<substrate_subxt::Error>() {
                    return Err(e);
                }
                let err_msg = format!("{:?}", e).to_lowercase();
                if err_msg.contains("restart") {
                    return Err(e);
                }
                log::error!(
                    target: DarwiniaEthereumTask::NAME,
                    "Failed verified redeem. [{}]: {}. {:?}",
                    tx.block_number,
                    tx.block_hash,
                    e
                );
                false
            }
        };
        if verified {
            tracker.finish(tx.block_number as usize)?;
            timing = SystemTime::now();
            continue;
        }

        if let Ok(elapsed) = timing.elapsed() {
            let secs = elapsed.as_secs();
            if secs >= task_config.check_timeout {
                tracker.finish(tx.block_number as usize)?;
                // todo: check timeout, skip thi transaction, write log
                continue;
            }
        }
        tokio::time::sleep(std::time::Duration::from_secs(task_config.interval_check)).await;
    }
}
