use lifeline::Sender;
use microkv::namespace::NamespaceMicroKV;

use bridge_traits::bridge::task::BridgeSand;

use crate::message::{Extrinsic, ToExtrinsicsMessage};
use crate::service::pangolin::types::ScanDataWrapper;
use crate::task::PangolinRopstenTask;

pub struct ScanScheduleMMRRootEvent<'a> {
    data: &'a mut ScanDataWrapper,
    microkv: NamespaceMicroKV,
}

impl<'a> ScanScheduleMMRRootEvent<'a> {
    pub fn new(data: &'a mut ScanDataWrapper, microkv: NamespaceMicroKV) -> Self {
        Self { data, microkv }
    }
}

impl<'a> ScanScheduleMMRRootEvent<'a> {
    pub async fn handle(&mut self) -> anyhow::Result<()> {
        let event = self
            .data
            .subquery
            .query_latest_schedule_mmr_root_event()
            .await?;
        if event.is_none() {
            log::info!(
                target: PangolinRopstenTask::NAME,
                "[pangolin] Not have more ScheduleMMRRootEvent"
            );
            return Ok(());
        }
        let latest = event.unwrap();
        if latest.emitted == 1 {
            log::info!(
                target: PangolinRopstenTask::NAME,
                "[pangolin] The latest ScheduleMMRRootEvent is emitted. event block is: {} and at block: {}. don't do this again.",
                latest.event_block_number,
                latest.at_block_number
            );
            return Ok(());
        } else {
            log::info!(
                target: PangolinRopstenTask::NAME,
                "[pangolin] Queried latest ScheduleMMRRootEvent event block is: {} and at block: {}",
                latest.event_block_number,
                latest.at_block_number
            );
        }

        let event_block_number = latest.event_block_number;

        let finalized_block_hash = self.data.darwinia.finalized_head().await?;
        let finalized_block_header_number = match self
            .data
            .darwinia
            .get_block_number_by_hash(finalized_block_hash)
            .await?
        {
            Some(v) => v,
            None => {
                log::warn!(
                    target: PangolinRopstenTask::NAME,
                    "[pangolin] Can not get last block header by finalized block hash: {}",
                    finalized_block_hash
                );
                return Ok(());
            }
        };

        if finalized_block_header_number < event_block_number {
            log::info!(
                target: PangolinRopstenTask::NAME,
                "[pangolin] The finalized block number ({}) less than event block number ({}). do nothing.",
                finalized_block_header_number,
                event_block_number
            );
            return Ok(());
        }

        let saved_latest: Option<u32> = self.microkv.get_as("latest_mmr_root_sign")?;
        if Some(event_block_number) == saved_latest {
            log::info!(
                target: PangolinRopstenTask::NAME,
                "[pangolin] This event block number ({}) is already submitted. Don't submit again.",
                event_block_number
            );
            return Ok(());
        }

        if !self
            .data
            .darwinia2ethereum
            .need_to_sign_mmr_root_of(
                &self.data.account,
                event_block_number,
                Some(finalized_block_header_number),
            )
            .await?
        {
            log::warn!(
                target: PangolinRopstenTask::NAME,
                "[pangolin] Don't need to sign mmr root for this event block: {} and at block: {}",
                latest.event_block_number,
                latest.at_block_number
            );
            return Ok(());
        }

        log::info!(
            target: PangolinRopstenTask::NAME,
            "[pangolin] Send sign mmr root for event block: {} and at block: {}",
            latest.event_block_number,
            latest.at_block_number
        );
        let sender = self.data.sender_to_extrinsics_mut();
        let ex = Extrinsic::SignAndSendMmrRoot(latest.event_block_number);
        sender.send(ToExtrinsicsMessage::Extrinsic(ex)).await?;

        self.microkv
            .put("latest_mmr_root_sign", &event_block_number)?;
        Ok(())
    }
}
