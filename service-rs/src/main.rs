mod types;
mod config;
mod models;
mod audio;
mod commands;
mod pipeline;
mod modes;
mod service;
mod dbus;

use std::sync::Arc;

use anyhow::Result;
use tokio::signal::unix::{signal, SignalKind};
use tracing::info;
use tracing_subscriber::EnvFilter;
use zbus::connection;

use crate::dbus::WillowDBus;
use crate::service::{ServiceCore, ServiceEvent};

const OBJECT_PATH: &str = "/com/github/saim/VoiceAssistant";
const BUS_NAME: &str = "com.github.saim.Willow";

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("willow_service=info".parse()?))
        .init();

    info!("Starting Willow Voice Assistant Service (Rust)");

    let core = Arc::new(ServiceCore::new()?);
    let dbus_service = WillowDBus::new(core.clone());

    let dbus_for_events = dbus_service.clone();
    let runtime = tokio::runtime::Handle::current();
    core.set_event_callback(Arc::new(move |event| {
        let dbus = dbus_for_events.clone();
        // Event relay runs on a std thread; spawn onto the Tokio runtime explicitly.
        runtime.spawn(async move {
            dbus.handle_event(event).await;
        });
    }));

    let _conn = connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, dbus_service.clone())?
        .build()
        .await?;

    let iface = _conn
        .object_server()
        .interface::<_, WillowDBus>(OBJECT_PATH)
        .await?;
    dbus_service.set_iface_ref(iface).await;

    core.auto_start();
    dbus_service
        .handle_event(ServiceEvent::StatusChanged)
        .await;

    info!("Willow Service running on D-Bus");
    info!("Bus name: {BUS_NAME}");
    info!("Object path: {OBJECT_PATH}");

    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;

    tokio::select! {
        _ = sigterm.recv() => info!("SIGTERM received"),
        _ = sigint.recv() => info!("SIGINT received"),
    }

    core.stop();
    info!("Service stopped successfully");
    Ok(())
}
