use crate::command_handler::LedCommand;
use anyhow::Result;
use esp_idf_svc::http::server::{Configuration, EspHttpConnection, EspHttpServer, Method, Request};
use esp_idf_svc::io::Write;
use std::sync::mpsc::Sender;

/// Simple HTTP interface that mirrors the BLE command set.
pub struct HttpServer<'a> {
    _server: EspHttpServer<'a>,
}

impl<'a> HttpServer<'a> {
    pub fn new(command_tx: Sender<LedCommand>) -> Result<Self> {
        let server_config = Configuration::default();
        let mut server = EspHttpServer::new(&server_config)?;

        Self::register_endpoints(&mut server, command_tx)?;

        log::info!("HTTP server started");
        Ok(Self { _server: server })
    }

    fn register_endpoints(server: &mut EspHttpServer<'a>, command_tx: Sender<LedCommand>) -> Result<()> {
        // GET / - API info
        server.fn_handler("/", Method::Get, |request| -> anyhow::Result<()> {
            log::info!("HTTP: GET / received");
            let mut response = request.into_ok_response()?;
            response.write_all(Self::api_overview().as_bytes())?;
            Ok(())
        })?;

        // POST /energy_bar?percentage=X
        let tx_energy_bar = command_tx.clone();
        server.fn_handler("/energy_bar", Method::Post, move |request| -> anyhow::Result<()> {
            match Self::get_query_param(&request, "percentage")
                .and_then(|p| p.parse::<u8>().ok())
                .filter(|&p| p <= 100)
            {
                Some(percentage) => {
                    log::info!("HTTP: energy_bar {}%", percentage);
                    if let Err(err) = tx_energy_bar.send(LedCommand::EnergyBar(percentage)) {
                        log::error!("Failed to enqueue energy_bar command: {}", err);
                        let mut response = request.into_status_response(500)?;
                        response.write_all(b"Internal server error")?;
                    } else {
                        let mut response = request.into_ok_response()?;
                        response.write_all(format!("Energy bar set to {}%", percentage).as_bytes())?;
                    }
                }
                None => {
                    let mut response = request.into_status_response(400)?;
                    response.write_all(b"Invalid percentage. Must be 0-100")?;
                }
            }
            Ok(())
        })?;

        // POST /set_power?power=X
        let tx_set_power = command_tx.clone();
        server.fn_handler("/set_power", Method::Post, move |request| -> anyhow::Result<()> {
            match Self::get_query_param(&request, "power")
                .and_then(|p| p.parse::<u8>().ok())
                .filter(|&p| p <= 100)
            {
                Some(power) => {
                    log::info!("HTTP: set_power {}%", power);
                    if let Err(err) = tx_set_power.send(LedCommand::EnergyBar(power)) {
                        log::error!("Failed to enqueue set_power command: {}", err);
                        let mut response = request.into_status_response(500)?;
                        response.write_all(b"Internal server error")?;
                    } else {
                        let mut response = request.into_ok_response()?;
                        response.write_all(format!("Power set to {}%", power).as_bytes())?;
                    }
                }
                None => {
                    let mut response = request.into_status_response(400)?;
                    response.write_all(b"Invalid power. Must be 0-100")?;
                }
            }
            Ok(())
        })?;

        // POST /energy_pulse
        let tx_energy_pulse = command_tx.clone();
        server.fn_handler("/energy_pulse", Method::Post, move |request| -> anyhow::Result<()> {
            log::info!("HTTP: energy_pulse");
            if let Err(err) = tx_energy_pulse.send(LedCommand::EnergyPulse) {
                log::error!("Failed to enqueue energy_pulse command: {}", err);
                let mut response = request.into_status_response(500)?;
                response.write_all(b"Internal server error")?;
            } else {
                let mut response = request.into_ok_response()?;
                response.write_all(b"Energy pulse triggered")?;
            }
            Ok(())
        })?;

        // POST /breathing
        let tx_breathing = command_tx.clone();
        server.fn_handler("/breathing", Method::Post, move |request| -> anyhow::Result<()> {
            log::info!("HTTP: breathing");
            if let Err(err) = tx_breathing.send(LedCommand::Breathing) {
                log::error!("Failed to enqueue breathing command: {}", err);
                let mut response = request.into_status_response(500)?;
                response.write_all(b"Internal server error")?;
            } else {
                let mut response = request.into_ok_response()?;
                response.write_all(b"Breathing effect started")?;
            }
            Ok(())
        })?;

        // POST /electricity
        let tx_electricity = command_tx.clone();
        server.fn_handler("/electricity", Method::Post, move |request| -> anyhow::Result<()> {
            log::info!("HTTP: electricity");
            if let Err(err) = tx_electricity.send(LedCommand::Electricity) {
                log::error!("Failed to enqueue electricity command: {}", err);
                let mut response = request.into_status_response(500)?;
                response.write_all(b"Internal server error")?;
            } else {
                let mut response = request.into_ok_response()?;
                response.write_all(b"Electricity effect started")?;
            }
            Ok(())
        })?;

        // POST /idle
        let tx_idle = command_tx;
        server.fn_handler("/idle", Method::Post, move |request| -> anyhow::Result<()> {
            log::info!("HTTP: idle");
            if let Err(err) = tx_idle.send(LedCommand::Idle) {
                log::error!("Failed to enqueue idle command: {}", err);
                let mut response = request.into_status_response(500)?;
                response.write_all(b"Internal server error")?;
            } else {
                let mut response = request.into_ok_response()?;
                response.write_all(b"Idle mode set")?;
            }
            Ok(())
        })?;

        Ok(())
    }

    fn get_query_param(request: &Request<&mut EspHttpConnection>, param: &str) -> Option<String> {
        let uri = request.uri();
        let query = uri.split_once('?').map(|(_, q)| q)?;

        url::form_urlencoded::parse(query.as_bytes())
            .find(|(key, _)| key == param)
            .map(|(_, value)| value.to_string())
    }

    fn api_overview() -> &'static str {
        r#"{
  "name": "ShadowLED Controller",
  "version": "1.0.0",
  "endpoints": {
    "GET /": "API information",
    "POST /energy_bar?percentage=50": "Set energy bar with percentage (0-100)",
    "POST /set_power?power=75": "Set power level (0-100)",
    "POST /energy_pulse": "Trigger energy pulse effect",
    "POST /breathing": "Start breathing effect",
    "POST /electricity": "Start electricity effect",
    "POST /idle": "Set idle mode"
  }
}"#
    }
}