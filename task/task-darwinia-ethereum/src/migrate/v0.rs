use microkv::namespace::NamespaceMicroKV;

use bridge_traits::bridge::task::BridgeSand;
use component_state::state::BridgeState;

use crate::task::DarwiniaEthereumTask;

pub fn migrate(state: &BridgeState) -> anyhow::Result<()> {
    let microkv = state.microkv_with_namespace(DarwiniaEthereumTask::NAME);
    migrate_scan_darwinia(&microkv)?;
    migrate_scan_ethereum(&microkv)?;
    Ok(())
}

fn migrate_scan_darwinia(microkv: &NamespaceMicroKV) -> anyhow::Result<()> {
    let key = "last-tracked-darwinia-block";
    let block_darwinia: Option<u64> = microkv.get_as(key)?;
    if let Some(block) = block_darwinia {
        microkv.put("scan.darwinia.next", &block)?;
        microkv.delete(key)?;
    }
    Ok(())
}

fn migrate_scan_ethereum(microkv: &NamespaceMicroKV) -> anyhow::Result<()> {
    let key = "last-redeemed";
    let block_ropsten: Option<u64> = microkv.get_as(key)?;
    if let Some(block) = block_ropsten {
        microkv.put("scan.ethereum.next", &block)?;
        microkv.delete(key)?;
    }
    Ok(())
}
