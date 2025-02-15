use microkv::namespace::NamespaceMicroKV;

use bridge_traits::bridge::task::BridgeSand;
use component_state::state::BridgeState;

use crate::task::PangolinRopstenTask;

pub fn migrate(state: &BridgeState) -> anyhow::Result<()> {
    let microkv = state.microkv_with_namespace(PangolinRopstenTask::NAME);
    migrate_scan_pangolin(&microkv)?;
    migrate_scan_opsten(&microkv)?;
    Ok(())
}

fn migrate_scan_pangolin(microkv: &NamespaceMicroKV) -> anyhow::Result<()> {
    let key = "last-tracked-pangolin-block";
    let block_pangolin: Option<u64> = microkv.get_as(key)?;
    if let Some(block) = block_pangolin {
        microkv.put("scan.pangolin.next", &block)?;
        microkv.delete(key)?;
    }
    Ok(())
}

fn migrate_scan_opsten(microkv: &NamespaceMicroKV) -> anyhow::Result<()> {
    let key = "last-redeemed-ropsten";
    let block_ropsten: Option<u64> = microkv.get_as(key)?;
    if let Some(block) = block_ropsten {
        microkv.put("scan.ropsten.next", &block)?;
        microkv.delete(key)?;
    }
    Ok(())
}
