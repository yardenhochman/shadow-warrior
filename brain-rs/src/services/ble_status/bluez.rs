//! BlueZ glue (Linux only): GATT application + LE advertisement via `bluer`.
//! Not exercised by unit tests (needs bluetoothd and an adapter); the logic it
//! publishes lives in the pure layer of the parent module.

use anyhow::{anyhow, Context, Result};
use bluer::{
    adv::{Advertisement, Type as AdvType},
    gatt::local::{
        Application, Characteristic, CharacteristicNotify, CharacteristicNotifyMethod,
        CharacteristicRead, Service,
    },
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::{broadcast, watch};
use tracing::{debug, info};
use uuid::Uuid;

use super::{
    chunk_frames, diff, encode, local_name, Encoded, SnapshotFn, DEFAULT_MTU, HEALTH_UUID, SERVICE_UUID,
    STATE_UUID, SUMMARY_UUID,
};
use crate::config::BleStatusConfig;
use crate::services::ble_manager::select_adapter;

type Shared = Arc<Mutex<Encoded>>;

fn uuid(s: &str) -> Uuid {
    Uuid::parse_str(s).expect("constant UUID is valid")
}

/// One read + notify characteristic backed by `get` (current value) and a
/// broadcast channel (changes). Deliberately no write handler: read-only.
/// Hook for later: add a `write: Some(CharacteristicWrite { .. })` characteristic
/// here for controls.
fn char_ro(
    id: &str,
    get: impl Fn() -> Vec<u8> + Send + Sync + Clone + 'static,
    tx: broadcast::Sender<Vec<u8>>,
    mtu_hint: Arc<AtomicUsize>,
) -> Characteristic {
    let get_read = get.clone();
    let mtu_read = mtu_hint.clone();
    Characteristic {
        uuid: uuid(id),
        read: Some(CharacteristicRead {
            read: true,
            fun: Box::new(move |req| {
                // bluer does not expose the MTU on the notifier, so remember the
                // largest MTU seen on a read request and use it for notifications.
                mtu_read.fetch_max(req.mtu as usize, Ordering::Relaxed);
                // BlueZ long reads ask for the value starting at `offset`.
                let v = get_read().into_iter().skip(req.offset as usize).collect::<Vec<u8>>();
                Box::pin(async move { Ok(v) })
            }),
            ..Default::default()
        }),
        notify: Some(CharacteristicNotify {
            notify: true,
            method: CharacteristicNotifyMethod::Fun(Box::new(move |mut notifier| {
                let mut rx = tx.subscribe();
                let get = get.clone();
                let mtu_hint = mtu_hint.clone();
                Box::pin(async move {
                    let mtu = mtu_hint.load(Ordering::Relaxed).max(DEFAULT_MTU);
                    // Subscribers get the current value right away, then changes.
                    let mut pending = Some(get());
                    loop {
                        let value = match pending.take() {
                            Some(v) => v,
                            None => match rx.recv().await {
                                Ok(v) => v,
                                Err(broadcast::error::RecvError::Lagged(_)) => get(),
                                Err(broadcast::error::RecvError::Closed) => break,
                            },
                        };
                        if notifier.is_stopped() {
                            break;
                        }
                        for frame in chunk_frames(&value, mtu) {
                            if let Err(e) = notifier.notify(frame).await {
                                debug!("BLE status notify ended: {e}");
                                return;
                            }
                        }
                    }
                })
            })),
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// Run until `stop` flips to true. Returns `Err` when BlueZ/adapter is
/// unavailable (caller logs and the daemon continues).
pub(super) async fn run(
    cfg: BleStatusConfig,
    ble_adapter: Option<String>,
    snapshot: SnapshotFn,
    mut stop: watch::Receiver<bool>,
) -> Result<()> {
    let session = bluer::Session::new().await.context("cannot reach bluetoothd (is BlueZ running?)")?;
    let names = session.adapter_names().await.context("listing Bluetooth adapters")?;
    let wanted = cfg.adapter.clone().or(ble_adapter);
    let idx = select_adapter(&names, wanted.as_deref()).map_err(|e| anyhow!(e))?;
    let adapter = session.adapter(&names[idx]).context("opening adapter")?;
    adapter.set_powered(true).await.context("powering adapter")?;
    let addr = adapter.address().await.context("reading adapter address")?;
    let name = local_name(&cfg.name_prefix, [addr.0[4], addr.0[5]]);

    let first = encode(&snapshot().await);
    let shared: Shared = Arc::new(Mutex::new(first));
    let (tx_state, _) = broadcast::channel::<Vec<u8>>(8);
    let (tx_summary, _) = broadcast::channel::<Vec<u8>>(8);
    let (tx_health, _) = broadcast::channel::<Vec<u8>>(8);

    let getter = |f: fn(&Encoded) -> &Vec<u8>| {
        let s = shared.clone();
        move || f(&s.lock().unwrap_or_else(|p| p.into_inner())).clone()
    };
    let mtu_hint = Arc::new(AtomicUsize::new(DEFAULT_MTU));
    let app = Application {
        services: vec![Service {
            uuid: uuid(SERVICE_UUID),
            primary: true,
            characteristics: vec![
                char_ro(STATE_UUID, getter(|e| &e.state), tx_state.clone(), mtu_hint.clone()),
                char_ro(SUMMARY_UUID, getter(|e| &e.summary), tx_summary.clone(), mtu_hint.clone()),
                char_ro(HEALTH_UUID, getter(|e| &e.health), tx_health.clone(), mtu_hint.clone()),
            ],
            ..Default::default()
        }],
        ..Default::default()
    };
    let app_handle = adapter.serve_gatt_application(app).await.context("registering GATT application")?;

    let adv = Advertisement {
        advertisement_type: AdvType::Peripheral,
        service_uuids: [uuid(SERVICE_UUID)].into_iter().collect(),
        discoverable: Some(true),
        local_name: Some(name.clone()),
        ..Default::default()
    };
    let adv_handle = adapter.advertise(adv).await.context("starting advertisement")?;
    info!("BLE status service advertising as {name} on {}", names[idx]);

    let mut tick = tokio::time::interval(Duration::from_millis(cfg.poll_ms.max(100)));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = stop.changed() => break,
            _ = tick.tick() => {
                let new = encode(&snapshot().await);
                let changed = {
                    let mut cur = shared.lock().unwrap_or_else(|p| p.into_inner());
                    let ch = diff(Some(&cur), &new);
                    if ch.any() {
                        *cur = new.clone();
                    }
                    ch
                };
                // send() errs only when nobody is subscribed: fine.
                if changed.state { let _ = tx_state.send(new.state.clone()); }
                if changed.summary { let _ = tx_summary.send(new.summary.clone()); }
                if changed.health { let _ = tx_health.send(new.health.clone()); }
            }
        }
    }
    // Dropping the handles unregisters the advertisement and application.
    drop(adv_handle);
    drop(app_handle);
    info!("BLE status service stopped");
    Ok(())
}
