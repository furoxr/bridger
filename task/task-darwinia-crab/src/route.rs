use std::str::FromStr;

use lifeline::{Bus, Receiver, Sender};

use bridge_traits::bridge::config::Config;
use bridge_traits::bridge::task::{BridgeSand, TaskTerminal};
use bridge_traits::error::StandardError;

use crate::bus::DarwiniaCrabBus;
use crate::config::DarwiniaCrabConfig;
use crate::message::{DarwiniaCrabMessageReceive, DarwiniaCrabMessageSend};
use crate::task::DarwiniaCrabTask;
use crate::types::BridgeName;

pub async fn dispatch_route(
    bus: &DarwiniaCrabBus,
    uri: String,
    param: serde_json::Value,
) -> anyhow::Result<TaskTerminal> {
    match &uri[..] {
        "init-bridge" => init_bridge(bus, param).await,
        "start-relay" => start_relay(bus, param).await,
        _ => Ok(TaskTerminal::new("Unsupported command")),
    }
}

fn bridge_name_from_param(param: &serde_json::Value) -> anyhow::Result<BridgeName> {
    let bridge_value = param
        .get("bridge")
        .ok_or_else(|| StandardError::Api("The bridge is required".to_string()))?;
    let bridge_text = bridge_value
        .as_str()
        .ok_or_else(|| StandardError::Api("Failed to get bridge".to_string()))?;
    BridgeName::from_str(bridge_text).map_err(|_e| {
        StandardError::Api(format!("Not support this bridge: {}", bridge_text)).into()
    })
}

#[allow(clippy::never_loop)]
async fn init_bridge(
    bus: &DarwiniaCrabBus,
    param: serde_json::Value,
) -> anyhow::Result<TaskTerminal> {
    let bridge_name = bridge_name_from_param(&param)?;
    let mut sender = bus.tx::<DarwiniaCrabMessageSend>()?;
    let mut receiver = bus.rx::<DarwiniaCrabMessageReceive>()?;

    sender
        .send(DarwiniaCrabMessageSend::InitBridge(bridge_name.clone()))
        .await?;

    while let Some(recv) = receiver.recv().await {
        match recv {
            DarwiniaCrabMessageReceive::FinishedInitBridge => break,
        }
    }

    Ok(TaskTerminal::new(format!(
        "init bridge {:?} success",
        bridge_name
    )))
}

async fn start_relay(
    bus: &DarwiniaCrabBus,
    _param: serde_json::Value,
) -> anyhow::Result<TaskTerminal> {
    let mut sender = bus.tx::<DarwiniaCrabMessageSend>()?;
    sender.send(DarwiniaCrabMessageSend::Relay).await?;

    let state_task = support_keep::state::get_state_task_unwrap(DarwiniaCrabTask::NAME)?;
    let mut config_task: DarwiniaCrabConfig = Config::load(state_task.config_path.clone())?;
    let mut config_relay = config_task.relay;
    config_relay.auto_start = true;
    config_task.relay = config_relay;
    Config::persist(
        &state_task.config_path,
        config_task,
        state_task.config_format.clone(),
    )?;

    Ok(TaskTerminal::new("success"))
}
