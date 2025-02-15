use relay_substrate_client::ChainBase;
use relay_utils::MaybeConnectionError;
use std::time::{SystemTime, UNIX_EPOCH};

use sp_runtime::{FixedPointNumber, FixedU128};

use bridge_traits::bridge::component::BridgeComponent;
use bridge_traits::bridge::task::BridgeSand;
use bridge_traits::error::StandardError;
use component_pangolin_s2s::PangolinChain;
use component_pangoro_s2s::PangoroChain;
use component_subscan::{Subscan, SubscanComponent};

use crate::fee::strategy::common::StrategyHelper;
use crate::fee::UpdateFeeStrategy;
use crate::task::PangolinPangoroTask;

const MIN_RELAY_FEE_PANGOLIN: u128 = 15 * drml_common_primitives::COIN;
const MIN_RELAY_FEE_PANGORO: u128 = 15 * drml_common_primitives::COIN;

#[derive(Clone)]
pub struct ReasonableStrategy {
    helper: StrategyHelper,
    subscan_pangolin: Subscan,
    subscan_pangoro: Subscan,
}

impl ReasonableStrategy {
    pub async fn new() -> anyhow::Result<Self> {
        let helper = StrategyHelper::new().await?;
        let component_subcan_pangolin = SubscanComponent::restore_with_namespace::<
            PangolinPangoroTask,
        >("pangolin".to_string())?;
        let component_subscan_pangoro =
            SubscanComponent::restore_with_namespace::<PangolinPangoroTask>("pangoro".to_string())?;
        let subscan_pangolin = component_subcan_pangolin.component().await?;
        let subscan_pangoro = component_subscan_pangoro.component().await?;
        Ok(Self {
            helper,
            subscan_pangolin,
            subscan_pangoro,
        })
    }
}

impl ReasonableStrategy {
    async fn _pangolin_open_price(&self) -> anyhow::Result<f64> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        match self.subscan_pangolin.price(now).await?.data() {
            Ok(Some(v)) => Ok(v.price),
            _ => {
                let subscan_dock = self
                    .subscan_pangolin
                    .clone()
                    .endpoint("https://dock.api.subscan.io");
                let open_price = subscan_dock.price(now).await?;
                let data = open_price.data()?;
                match data {
                    Some(v) => Ok(v.price),
                    None => anyhow::bail!("Can not query pangolin price"),
                }
            }
        }
    }

    async fn _pangoro_open_price(&self) -> anyhow::Result<f64> {
        let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
        match self.subscan_pangoro.price(now).await?.data() {
            Ok(Some(v)) => Ok(v.price),
            _ => {
                let subscan_darwinia = self
                    .subscan_pangoro
                    .clone()
                    .endpoint("https://darwinia.api.subscan.io");
                let open_price = subscan_darwinia.price(now).await?;
                let data = open_price.data()?;
                match data {
                    Some(v) => Ok(v.price),
                    None => anyhow::bail!("Can not query pangoro price"),
                }
            }
        }
    }

    async fn conversion_pangolin_to_pangoro(
        &self,
        pangolin_currency: drml_common_primitives::Balance,
    ) -> anyhow::Result<drml_common_primitives::Balance> {
        let price_pangolin = self._pangolin_open_price().await?;
        let price_pangoro = self._pangoro_open_price().await?;
        let rate = price_pangolin / price_pangoro;
        let rate = FixedU128::from_float(rate);
        let ret = rate.saturating_mul_int(pangolin_currency);
        Ok(ret)
    }
    async fn conversion_pangoro_to_pangolin(
        &self,
        pangoro_currency: drml_common_primitives::Balance,
    ) -> anyhow::Result<drml_common_primitives::Balance> {
        let price_pangolin = self._pangolin_open_price().await?;
        let price_pangoro = self._pangoro_open_price().await?;
        let rate = price_pangoro / price_pangolin;
        let rate = FixedU128::from_float(rate);
        let ret = rate.saturating_mul_int(pangoro_currency);
        Ok(ret)
    }
}

impl ReasonableStrategy {
    async fn update_pangolin_fee(
        &self,
        expect_fee_pangolin: <PangolinChain as ChainBase>::Balance,
    ) -> anyhow::Result<()> {
        log::info!(
            target: PangolinPangoroTask::NAME,
            "[reasonable] Update pangolin fee: {}",
            expect_fee_pangolin
        );
        let pangolin_signer = self.helper.pangolin_signer().clone();
        let pangolin_api = self.helper.pangolin_api();
        pangolin_api
            .update_relay_fee(pangolin_signer.clone(), expect_fee_pangolin)
            .await
    }

    async fn update_pangoro_fee(
        &self,
        expect_fee_pangoro: <PangoroChain as ChainBase>::Balance,
    ) -> anyhow::Result<()> {
        log::info!(
            target: PangolinPangoroTask::NAME,
            "[reasonable] Update pangoro fee: {}",
            expect_fee_pangoro
        );
        let pangoro_signer = self.helper.pangoro_signer().clone();
        let pangoro_api = self.helper.pangoro_api();
        pangoro_api
            .update_relay_fee(pangoro_signer.clone(), expect_fee_pangoro)
            .await
    }
}

#[async_trait::async_trait]
impl UpdateFeeStrategy for ReasonableStrategy {
    async fn handle(&mut self) -> anyhow::Result<()> {
        let top100_pangolin = self.subscan_pangolin.extrinsics(1, 100).await?;
        let top100_pangoro = self.subscan_pangoro.extrinsics(1, 100).await?;
        let top100_pangolin = top100_pangolin.data()?.ok_or_else(|| {
            StandardError::Api("Can not query pangolin extrinsics data".to_string())
        })?;
        let top100_pangoro = top100_pangoro.data()?.ok_or_else(|| {
            StandardError::Api("Can not query pangoro extrinsics data".to_string())
        })?;

        let max_fee_pangolin = top100_pangolin
            .extrinsics
            .unwrap_or_default()
            .iter()
            .map(|item| item.fee)
            .max()
            .unwrap_or(0);
        let max_fee_pangoro = top100_pangoro
            .extrinsics
            .unwrap_or_default()
            .iter()
            .map(|item| item.fee)
            .max()
            .unwrap_or(0);

        let top100_max_cost_pangolin =
            max_fee_pangolin + self.conversion_pangoro_to_pangolin(max_fee_pangoro).await?;
        let top100_max_cost_pangoro = max_fee_pangoro
            + self
                .conversion_pangolin_to_pangoro(max_fee_pangolin)
                .await?;

        // Nice (
        let expect_fee_pangolin = MIN_RELAY_FEE_PANGOLIN + (top100_max_cost_pangolin * 15);
        let expect_fee_pangoro = MIN_RELAY_FEE_PANGORO + (top100_max_cost_pangoro * 15);

        let mut times = 0;
        loop {
            times += 1;
            if times > 3 {
                log::error!(
                    target: PangolinPangoroTask::NAME,
                    "[pangoro] Try reconnect many times({}), skip update fee (update fee strategy reasonable)",
                    times
                );
                break;
            }
            match self.update_pangoro_fee(expect_fee_pangoro).await {
                Ok(_) => break,
                Err(e) => {
                    if let Some(client_error) = e.downcast_ref::<relay_substrate_client::Error>() {
                        if client_error.is_connection_error() {
                            log::debug!(
                                "[pangoro] Try reconnect to chain (update fee strategy reasonable)"
                            );
                            if let Err(re) = self.helper.reconnect_pangoro().await {
                                log::error!(
                                    "[pangoro] Failed to reconnect substrate client: {:?} (update fee strategy)",
                                    re
                                );
                                continue;
                            }
                        }
                    }
                }
            }
        }

        times = 0;
        loop {
            times += 1;
            if times > 3 {
                log::error!(
                    target: PangolinPangoroTask::NAME,
                    "[pangolin] Try reconnect many times({}), skip update fee (update fee strategy reasonable)",
                    times
                );
                break;
            }

            match self.update_pangolin_fee(expect_fee_pangolin).await {
                Ok(_) => break,
                Err(e) => {
                    if let Some(client_error) = e.downcast_ref::<relay_substrate_client::Error>() {
                        if client_error.is_connection_error() {
                            log::debug!(
                                "[pangolin] Try reconnect to chain (update fee strategy reasonable)"
                            );
                            if let Err(re) = self.helper.reconnect_pangolin().await {
                                log::error!(
                                    "[pangolin] Failed to reconnect substrate client: {:?} (update fee strategy)",
                                    re
                                );
                                continue;
                            }
                        }
                    }
                }
            }
        }

        Ok(())
    }
}
