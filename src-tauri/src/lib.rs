use std::{
    collections::HashMap,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use regex::Regex;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tauri::{AppHandle, Emitter, State};
use tokio::sync::{oneshot, Mutex};
use url::Url;

const GLYMPSE_API_BASE: &str = "https://api.glympse.com/v2/";
const GLYMPSE_VIEWER_API_KEY: &str = "0SLq661pXHmqdWgI8Yb1";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeSettings {
    pub glympse_source: String,
    pub caltopo_connect_key: String,
    pub poll_interval_secs: u64,
    pub forward_unchanged: bool,
    pub include_altitude: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LocationFix {
    pub lat: f64,
    pub lng: f64,
    pub accuracy: Option<f64>,
    pub altitude: Option<f64>,
    pub speed: Option<f64>,
    pub heading: Option<f64>,
    pub timestamp_ms: Option<u64>,
    pub source_label: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeStatus {
    pub running: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BridgeLog {
    pub level: String,
    pub message: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ForwardEvent {
    pub caltopo_id: String,
    pub source_label: Option<String>,
    pub lat: f64,
    pub lng: f64,
    pub status: String,
    pub message: String,
    pub timestamp_ms: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollOutcome {
    pub location: Option<LocationFix>,
    pub forward: Option<ForwardEvent>,
    pub locations: Vec<LocationFix>,
    pub forwards: Vec<ForwardEvent>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlympseDiagnostics {
    pub extracted_code: Option<String>,
    pub code_variants: Vec<String>,
    pub attempts: Vec<GlympseAttempt>,
    pub parsed_location: Option<LocationFix>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlympseAttempt {
    pub url: String,
    pub status: Option<u16>,
    pub ok: bool,
    pub content_type: Option<String>,
    pub parsed: bool,
    pub message: String,
    pub response_preview: Option<String>,
}

#[derive(Debug, Default)]
struct PollContext {
    viewer_token: Option<String>,
    last_signatures: HashMap<String, String>,
}

struct BridgeRuntime {
    stop_tx: oneshot::Sender<()>,
    handle: tauri::async_runtime::JoinHandle<()>,
}

pub struct AppState {
    runtime: Mutex<Option<BridgeRuntime>>,
    http: Client,
}

impl AppState {
    fn new() -> Self {
        Self {
            runtime: Mutex::new(None),
            http: Client::builder()
                .timeout(Duration::from_secs(15))
                .user_agent("Glympse CalTopo Bridge/0.1")
                .build()
                .expect("reqwest client should build"),
        }
    }
}

#[tauri::command]
async fn start_bridge(
    app: AppHandle,
    state: State<'_, AppState>,
    settings: BridgeSettings,
) -> Result<(), String> {
    validate_settings(&settings)?;

    let mut runtime = state.runtime.lock().await;
    if runtime.is_some() {
        return Err("Bridge is already running".to_string());
    }

    let (stop_tx, stop_rx) = oneshot::channel();
    let http = state.http.clone();
    let app_for_task = app.clone();
    let interval = settings.poll_interval_secs.max(2);

    emit_status(&app, true, format!("Running. Polling every {interval}s"));
    emit_log(
        &app,
        "info",
        "Started Glympse -> CalTopo bridge; Glympse names will be used for live tracks",
    );

    let handle = tauri::async_runtime::spawn(async move {
        run_bridge(app_for_task, http, settings, stop_rx).await;
    });

    *runtime = Some(BridgeRuntime { stop_tx, handle });
    Ok(())
}

#[tauri::command]
async fn stop_bridge(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let runtime = {
        let mut runtime = state.runtime.lock().await;
        runtime.take()
    };

    if let Some(runtime) = runtime {
        let _ = runtime.stop_tx.send(());
        let _ = runtime.handle.await;
        emit_status(&app, false, "Stopped");
        emit_log(&app, "info", "Bridge stopped");
    }

    Ok(())
}

#[tauri::command]
async fn get_status(state: State<'_, AppState>) -> Result<BridgeStatus, String> {
    let running = state.runtime.lock().await.is_some();
    Ok(BridgeStatus {
        running,
        message: if running {
            "Running".to_string()
        } else {
            "Stopped".to_string()
        },
    })
}

#[tauri::command]
async fn test_glympse(
    settings: BridgeSettings,
    state: State<'_, AppState>,
) -> Result<Vec<LocationFix>, String> {
    let mut context = PollContext::default();
    let fetch = fetch_glympse_locations(&state.http, &settings, &mut context).await?;
    Ok(fetch.locations)
}

#[tauri::command]
async fn diagnose_glympse(
    settings: BridgeSettings,
    state: State<'_, AppState>,
) -> Result<GlympseDiagnostics, String> {
    Ok(run_glympse_diagnostics(&state.http, &settings).await)
}

#[tauri::command]
async fn poll_once(
    app: AppHandle,
    settings: BridgeSettings,
    state: State<'_, AppState>,
) -> Result<PollOutcome, String> {
    validate_settings(&settings)?;
    let mut context = PollContext::default();
    let outcome = poll_and_forward(&state.http, &settings, &mut context).await?;
    emit_outcome(&app, &outcome);
    Ok(outcome)
}

async fn run_bridge(
    app: AppHandle,
    http: Client,
    settings: BridgeSettings,
    mut stop_rx: oneshot::Receiver<()>,
) {
    let mut context = PollContext::default();
    let interval = settings.poll_interval_secs.max(2);

    loop {
        match poll_and_forward(&http, &settings, &mut context).await {
            Ok(outcome) => emit_outcome(&app, &outcome),
            Err(error) => emit_log(&app, "error", error),
        }

        tokio::select! {
            _ = &mut stop_rx => {
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(interval)) => {}
        }
    }
}

async fn poll_and_forward(
    http: &Client,
    settings: &BridgeSettings,
    context: &mut PollContext,
) -> Result<PollOutcome, String> {
    let fetch = fetch_glympse_locations(http, settings, context).await?;
    let mut forwards = Vec::new();

    for location in &fetch.locations {
        let caltopo_id = match caltopo_id_for_location(location) {
            Ok(caltopo_id) => caltopo_id,
            Err(error) => {
                forwards.push(ForwardEvent {
                    caltopo_id: "not-forwarded".to_string(),
                    source_label: location.source_label.clone(),
                    lat: location.lat,
                    lng: location.lng,
                    status: "failed".to_string(),
                    message: error,
                    timestamp_ms: now_ms(),
                });
                continue;
            }
        };
        let signature = location_signature(location);

        if !settings.forward_unchanged
            && context
                .last_signatures
                .get(&caltopo_id)
                .is_some_and(|previous| previous == &signature)
        {
            forwards.push(ForwardEvent {
                caltopo_id,
                source_label: location.source_label.clone(),
                lat: location.lat,
                lng: location.lng,
                status: "skipped".to_string(),
                message: "Location unchanged since last forwarded fix".to_string(),
                timestamp_ms: now_ms(),
            });
            continue;
        }

        let forward = forward_to_caltopo(http, settings, location, &caltopo_id).await;
        if forward.status == "sent" {
            context.last_signatures.insert(caltopo_id, signature);
        }
        forwards.push(forward);
    }

    let sent = forwards
        .iter()
        .filter(|event| event.status == "sent")
        .count();
    let skipped = forwards
        .iter()
        .filter(|event| event.status == "skipped")
        .count();
    let failed = forwards
        .iter()
        .filter(|event| event.status == "failed")
        .count();
    let message = match (sent, skipped, failed) {
        (0, 0, 0) => "No active Glympse users were found".to_string(),
        (_, 0, 0) => format!("Forwarded {sent} active Glympse user(s) to CalTopo"),
        _ => format!("Processed {sent} sent, {skipped} skipped, {failed} failed CalTopo report(s)"),
    };

    Ok(PollOutcome {
        location: fetch.locations.first().cloned(),
        forward: forwards.first().cloned(),
        locations: fetch.locations,
        forwards,
        message,
    })
}

struct GlympseFetch {
    locations: Vec<LocationFix>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct GlympseInvite {
    code: String,
    label: Option<String>,
}

async fn fetch_glympse_locations(
    http: &Client,
    settings: &BridgeSettings,
    context: &mut PollContext,
) -> Result<GlympseFetch, String> {
    let mut errors = Vec::new();
    if context.viewer_token.is_none() {
        match request_glympse_viewer_token(http).await {
            Ok(token) => context.viewer_token = Some(token),
            Err(error) => errors.push(error),
        }
    }

    let urls = build_glympse_request_urls(
        &settings.glympse_source,
        None,
        context.viewer_token.as_deref(),
    );
    if urls.is_empty() {
        return Err("Enter a Glympse share URL or invite code".to_string());
    }

    let mut member_invites = Vec::new();
    let mut locations = Vec::new();
    let mut index = 0;
    while index < urls.len() {
        let url = urls[index].clone();
        index += 1;

        match glympse_get(http, &url, context.viewer_token.as_deref())
            .send()
            .await
        {
            Ok(response) => {
                let status = response.status();
                let text = response
                    .text()
                    .await
                    .map_err(|error| format!("Failed reading Glympse response: {error}"))?;
                if !status.is_success() {
                    errors.push(format!("{} returned HTTP {status}", redact_url(&url)));
                    continue;
                }

                member_invites.extend(extract_group_invites(&text));

                match parse_glympse_response(&text) {
                    Ok((location, _next_token)) => {
                        merge_location(&mut locations, location);
                    }
                    Err(error) => errors.push(format!("{}: {error}", redact_url(&url))),
                }
            }
            Err(error) => errors.push(format!("{}: {error}", redact_url(&url))),
        }
    }

    let member_fetch = fetch_group_member_locations(
        http,
        dedupe_invites(member_invites),
        context.viewer_token.as_deref(),
    )
    .await;
    errors.extend(member_fetch.errors);
    for location in member_fetch.locations {
        merge_location(&mut locations, location);
    }

    if locations.is_empty() {
        Err(format!(
            "Could not read a location from the Glympse source. {}",
            errors.join(" | ")
        ))
    } else {
        Ok(GlympseFetch { locations })
    }
}

struct MemberFetch {
    locations: Vec<LocationFix>,
    errors: Vec<String>,
}

async fn fetch_group_member_locations(
    http: &Client,
    invites: Vec<GlympseInvite>,
    viewer_token: Option<&str>,
) -> MemberFetch {
    if invites.is_empty() {
        return MemberFetch {
            locations: Vec::new(),
            errors: Vec::new(),
        };
    }

    let mut locations = Vec::new();
    let mut errors = Vec::new();

    for chunk in invites.chunks(16) {
        let mut tasks = Vec::new();
        for invite in chunk {
            let http = http.clone();
            let invite = invite.clone();
            let viewer_token = viewer_token.map(ToString::to_string);
            tasks.push(tauri::async_runtime::spawn(async move {
                fetch_group_member_location(&http, invite, viewer_token.as_deref()).await
            }));
        }

        for task in tasks {
            match task.await {
                Ok(member_fetch) => {
                    errors.extend(member_fetch.errors);
                    for location in member_fetch.locations {
                        merge_location(&mut locations, location);
                    }
                }
                Err(error) => errors.push(format!("Glympse member lookup task failed: {error}")),
            }
        }
    }

    MemberFetch { locations, errors }
}

async fn fetch_group_member_location(
    http: &Client,
    invite: GlympseInvite,
    viewer_token: Option<&str>,
) -> MemberFetch {
    let mut errors = Vec::new();
    for url in build_glympse_member_request_urls(&invite.code, viewer_token) {
        match glympse_get(http, &url, viewer_token).send().await {
            Ok(response) => {
                let status = response.status();
                let text = match response.text().await {
                    Ok(text) => text,
                    Err(error) => {
                        errors.push(format!(
                            "Failed reading Glympse member response for {}: {error}",
                            invite.code
                        ));
                        continue;
                    }
                };
                if !status.is_success() {
                    errors.push(format!("{} returned HTTP {status}", redact_url(&url)));
                    continue;
                }

                match parse_glympse_response(&text) {
                    Ok((mut location, _next_token)) => {
                        apply_source_label_hint(&mut location, invite.label.as_deref());
                        return MemberFetch {
                            locations: vec![location],
                            errors,
                        };
                    }
                    Err(error) => errors.push(format!("{}: {error}", redact_url(&url))),
                }
            }
            Err(error) => errors.push(format!("{}: {error}", redact_url(&url))),
        }
    }

    MemberFetch {
        locations: Vec::new(),
        errors,
    }
}

async fn forward_to_caltopo(
    http: &Client,
    settings: &BridgeSettings,
    location: &LocationFix,
    caltopo_id: &str,
) -> ForwardEvent {
    let url = build_caltopo_url(settings, location, caltopo_id);
    let timestamp_ms = now_ms();
    match http.get(url).send().await {
        Ok(response) => {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            let body_summary = body.trim().chars().take(180).collect::<String>();
            if status.is_success() {
                ForwardEvent {
                    caltopo_id: caltopo_id.to_string(),
                    source_label: location.source_label.clone(),
                    lat: location.lat,
                    lng: location.lng,
                    status: "sent".to_string(),
                    message: format!("CalTopo accepted the fix with HTTP {status}."),
                    timestamp_ms,
                }
            } else {
                ForwardEvent {
                    caltopo_id: caltopo_id.to_string(),
                    source_label: location.source_label.clone(),
                    lat: location.lat,
                    lng: location.lng,
                    status: "failed".to_string(),
                    message: format!("CalTopo returned HTTP {status}: {body_summary}"),
                    timestamp_ms,
                }
            }
        }
        Err(error) => ForwardEvent {
            caltopo_id: caltopo_id.to_string(),
            source_label: location.source_label.clone(),
            lat: location.lat,
            lng: location.lng,
            status: "failed".to_string(),
            message: format!("CalTopo request failed: {error}"),
            timestamp_ms,
        },
    }
}

fn validate_settings(settings: &BridgeSettings) -> Result<(), String> {
    if settings.glympse_source.trim().is_empty() {
        return Err("Glympse share URL or invite code is required".to_string());
    }
    if settings.caltopo_connect_key.trim().is_empty() {
        return Err("CalTopo live-track connect key is required".to_string());
    }
    Ok(())
}

async fn request_glympse_viewer_token(http: &Client) -> Result<String, String> {
    let url = build_glympse_login_url();
    let response = http
        .get(&url)
        .send()
        .await
        .map_err(|error| format!("Glympse viewer login failed: {error}"))?;
    let status = response.status();
    let text = response
        .text()
        .await
        .map_err(|error| format!("Failed reading Glympse viewer login response: {error}"))?;

    if !status.is_success() {
        return Err(format!(
            "Glympse viewer login returned HTTP {status}: {}",
            response_preview(&text)
        ));
    }

    let value: Value = serde_json::from_str(&text)
        .map_err(|error| format!("Glympse viewer login did not return JSON: {error}"))?;
    let token = value
        .pointer("/response/access_token")
        .and_then(Value::as_str)
        .filter(|token| !token.trim().is_empty())
        .ok_or_else(|| {
            "Glympse viewer login response did not include an access token".to_string()
        })?;

    Ok(token.to_string())
}

fn build_glympse_login_url() -> String {
    format!(
        "{GLYMPSE_API_BASE}account/login?username=viewer&password=password&api_key={GLYMPSE_VIEWER_API_KEY}"
    )
}

fn build_glympse_request_urls(
    source: &str,
    next_token: Option<&str>,
    oauth_token: Option<&str>,
) -> Vec<String> {
    let trimmed = source.trim();
    let Some(code) = extract_glympse_code(trimmed) else {
        return Vec::new();
    };

    let mut urls = Vec::new();
    for variant in glympse_code_variants(&code) {
        let encoded_code = urlencoding::encode(&variant);
        if oauth_token
            .filter(|value| !value.trim().is_empty())
            .is_some()
        {
            let next = next_token
                .filter(|value| !value.trim().is_empty())
                .unwrap_or("0");
            urls.push(format!(
                "{GLYMPSE_API_BASE}invites/{encoded_code}?next={}&debug=true",
                urlencoding::encode(next)
            ));
            if variant.starts_with('!') {
                let tag = variant.trim_start_matches('!').trim();
                if !tag.is_empty() {
                    let encoded_tag = urlencoding::encode(tag);
                    urls.push(format!(
                        "{GLYMPSE_API_BASE}groups/{encoded_tag}?branding=true"
                    ));
                    urls.push(format!(
                        "{GLYMPSE_API_BASE}groups/{encoded_tag}/events?next=0"
                    ));
                }
            }
        }
        urls.push(format!("{GLYMPSE_API_BASE}invites/{encoded_code}"));
        urls.push(format!(
            "{GLYMPSE_API_BASE}invites/{encoded_code}?locale=en_US&region=en_US"
        ));
    }

    if let Ok(url) = Url::parse(trimmed) {
        if url.scheme().starts_with("http") {
            urls.push(url.to_string());
        }
    }

    dedupe_strings(urls)
}

fn glympse_get<'a>(
    http: &'a Client,
    url: &'a str,
    viewer_token: Option<&'a str>,
) -> reqwest::RequestBuilder {
    let request = http.get(url);
    if url.starts_with(GLYMPSE_API_BASE) {
        if let Some(token) = viewer_token.filter(|token| !token.trim().is_empty()) {
            return request.bearer_auth(token);
        }
    }
    request
}

fn glympse_code_variants(code: &str) -> Vec<String> {
    let trimmed = code.trim();
    let without_bang = trimmed.trim_start_matches('!');
    let without_slashes = without_bang.trim_matches('/');
    let mut variants = vec![trimmed.to_string()];
    if without_bang != trimmed {
        variants.push(without_bang.to_string());
    }
    if without_slashes != without_bang {
        variants.push(without_slashes.to_string());
    }
    dedupe_strings(
        variants
            .into_iter()
            .filter(|variant| !variant.trim().is_empty())
            .collect(),
    )
}

fn dedupe_strings(values: Vec<String>) -> Vec<String> {
    let mut deduped = Vec::new();
    for value in values {
        if !deduped.contains(&value) {
            deduped.push(value);
        }
    }
    deduped
}

fn build_glympse_member_request_urls(invite_code: &str, viewer_token: Option<&str>) -> Vec<String> {
    let trimmed = invite_code.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }

    let encoded_code = urlencoding::encode(trimmed);
    if viewer_token
        .filter(|value| !value.trim().is_empty())
        .is_some()
    {
        vec![format!(
            "{GLYMPSE_API_BASE}invites/{encoded_code}?next=0&debug=true"
        )]
    } else {
        vec![format!("{GLYMPSE_API_BASE}invites/{encoded_code}")]
    }
}

fn dedupe_invites(invites: Vec<GlympseInvite>) -> Vec<GlympseInvite> {
    let mut deduped: Vec<GlympseInvite> = Vec::new();
    for invite in invites {
        if let Some(existing) = deduped
            .iter_mut()
            .find(|candidate| candidate.code == invite.code)
        {
            if existing.label.is_none() && invite.label.is_some() {
                existing.label = invite.label;
            }
        } else {
            deduped.push(invite);
        }
    }
    deduped
}

fn extract_group_invites(text: &str) -> Vec<GlympseInvite> {
    let Ok(value) = serde_json::from_str::<Value>(text) else {
        return Vec::new();
    };

    let mut invites = Vec::new();
    if let Some(response) = value.get("response").or_else(|| value.get("result")) {
        collect_group_invites(response, &mut invites);
    }
    dedupe_invites(invites)
}

fn collect_group_invites(value: &Value, out: &mut Vec<GlympseInvite>) {
    let Some(map) = value.as_object() else {
        return;
    };

    if let Some(members) = map.get("members").and_then(Value::as_array) {
        for member in members {
            if let Some(invite) = member
                .get("invite")
                .and_then(Value::as_str)
                .filter(|invite| !invite.trim().is_empty())
            {
                out.push(GlympseInvite {
                    code: invite.trim().to_string(),
                    label: glympse_member_label(member),
                });
            }
        }
    }

    if let Some(items) = map.get("items").and_then(Value::as_array) {
        for item in items {
            let item_type = item.get("type").and_then(Value::as_str).unwrap_or_default();
            let is_invite_item = item_type.eq_ignore_ascii_case("invite")
                || item_type.eq_ignore_ascii_case("swap")
                || item_type.is_empty();
            if is_invite_item {
                if let Some(invite) = item
                    .get("invite")
                    .and_then(Value::as_str)
                    .filter(|invite| !invite.trim().is_empty())
                {
                    out.push(GlympseInvite {
                        code: invite.trim().to_string(),
                        label: glympse_member_label(item),
                    });
                }
            }
        }
    }
}

fn glympse_member_label(value: &Value) -> Option<String> {
    for key in [
        "nickname",
        "name",
        "displayName",
        "display_name",
        "label",
        "title",
    ] {
        if let Some(label) = value
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|label| !label.is_empty())
        {
            return Some(label.to_string());
        }
    }

    value
        .get("user")
        .and_then(glympse_member_label)
        .or_else(|| value.get("profile").and_then(glympse_member_label))
}

async fn run_glympse_diagnostics(http: &Client, settings: &BridgeSettings) -> GlympseDiagnostics {
    let source = settings.glympse_source.trim();
    let extracted_code = extract_glympse_code(source);
    let code_variants = extracted_code
        .as_deref()
        .map(glympse_code_variants)
        .unwrap_or_default();

    let mut attempts = Vec::new();
    let oauth_token = match run_glympse_login_diagnostics(http).await {
        (Some(token), attempt) => {
            attempts.push(attempt);
            Some(token)
        }
        (None, attempt) => {
            attempts.push(attempt);
            None
        }
    };
    let urls = build_glympse_request_urls(source, None, oauth_token.as_deref());

    if urls.is_empty() {
        return GlympseDiagnostics {
            extracted_code,
            code_variants,
            attempts,
            parsed_location: None,
            summary: "No Glympse source was entered".to_string(),
        };
    }

    let mut parsed_location = None;
    let mut member_invites = Vec::new();
    let mut index = 0;
    while index < urls.len() {
        let url = urls[index].clone();
        index += 1;

        match glympse_get(http, &url, oauth_token.as_deref()).send().await {
            Ok(response) => {
                let status = response.status();
                let content_type = response
                    .headers()
                    .get(reqwest::header::CONTENT_TYPE)
                    .and_then(|value| value.to_str().ok())
                    .map(ToString::to_string);
                match response.text().await {
                    Ok(text) => {
                        let discovered_invites = extract_group_invites(&text);
                        let discovered_count = discovered_invites.len();
                        member_invites.extend(discovered_invites);

                        match parse_glympse_response(&text) {
                            Ok((location, _)) => {
                                if parsed_location.is_none() {
                                    parsed_location = Some(location);
                                }
                                attempts.push(GlympseAttempt {
                                    url: redact_url(&url),
                                    status: Some(status.as_u16()),
                                    ok: status.is_success(),
                                    content_type,
                                    parsed: true,
                                    message: "Parsed a location from this response".to_string(),
                                    response_preview: Some(response_preview(&text)),
                                });
                            }
                            Err(error) => {
                                let message = if discovered_count > 0 {
                                    format!(
                                        "{error}; queued {discovered_count} active member invite lookup(s)"
                                    )
                                } else {
                                    error
                                };
                                attempts.push(GlympseAttempt {
                                    url: redact_url(&url),
                                    status: Some(status.as_u16()),
                                    ok: status.is_success(),
                                    content_type,
                                    parsed: false,
                                    message,
                                    response_preview: Some(response_preview(&text)),
                                });
                            }
                        }
                    }
                    Err(error) => attempts.push(GlympseAttempt {
                        url: redact_url(&url),
                        status: Some(status.as_u16()),
                        ok: status.is_success(),
                        content_type,
                        parsed: false,
                        message: format!("Failed reading response body: {error}"),
                        response_preview: None,
                    }),
                }
            }
            Err(error) => attempts.push(GlympseAttempt {
                url: redact_url(&url),
                status: None,
                ok: false,
                content_type: None,
                parsed: false,
                message: error.to_string(),
                response_preview: None,
            }),
        }
    }

    for invite in dedupe_invites(member_invites) {
        for url in build_glympse_member_request_urls(&invite.code, oauth_token.as_deref()) {
            match glympse_get(http, &url, oauth_token.as_deref()).send().await {
                Ok(response) => {
                    let status = response.status();
                    let content_type = response
                        .headers()
                        .get(reqwest::header::CONTENT_TYPE)
                        .and_then(|value| value.to_str().ok())
                        .map(ToString::to_string);
                    match response.text().await {
                        Ok(text) => match parse_glympse_response(&text) {
                            Ok((mut location, _)) => {
                                apply_source_label_hint(&mut location, invite.label.as_deref());
                                if parsed_location.is_none() {
                                    parsed_location = Some(location);
                                }
                                attempts.push(GlympseAttempt {
                                    url: redact_url(&url),
                                    status: Some(status.as_u16()),
                                    ok: status.is_success(),
                                    content_type,
                                    parsed: true,
                                    message: match &invite.label {
                                        Some(label) => {
                                            format!("Parsed a location for group member {label}")
                                        }
                                        None => "Parsed a location from a group member invite"
                                            .to_string(),
                                    },
                                    response_preview: Some(response_preview(&text)),
                                });
                            }
                            Err(error) => attempts.push(GlympseAttempt {
                                url: redact_url(&url),
                                status: Some(status.as_u16()),
                                ok: status.is_success(),
                                content_type,
                                parsed: false,
                                message: error,
                                response_preview: Some(response_preview(&text)),
                            }),
                        },
                        Err(error) => attempts.push(GlympseAttempt {
                            url: redact_url(&url),
                            status: Some(status.as_u16()),
                            ok: status.is_success(),
                            content_type,
                            parsed: false,
                            message: format!("Failed reading response body: {error}"),
                            response_preview: None,
                        }),
                    }
                }
                Err(error) => attempts.push(GlympseAttempt {
                    url: redact_url(&url),
                    status: None,
                    ok: false,
                    content_type: None,
                    parsed: false,
                    message: error.to_string(),
                    response_preview: None,
                }),
            }
        }
    }

    let summary = if parsed_location.is_some() {
        "At least one Glympse response contained a usable location".to_string()
    } else if attempts
        .iter()
        .any(|attempt| attempt.message.contains("active member invite lookup"))
    {
        "The Glympse tag listed active member invites, but no member feed contained coordinates"
            .to_string()
    } else if attempts
        .iter()
        .any(|attempt| attempt.message.contains("no active shared positions"))
    {
        "The Glympse tag is reachable, but no one is actively sharing a position right now"
            .to_string()
    } else {
        "No attempted Glympse response contained recognizable coordinates".to_string()
    };

    GlympseDiagnostics {
        extracted_code,
        code_variants,
        attempts,
        parsed_location,
        summary,
    }
}

async fn run_glympse_login_diagnostics(http: &Client) -> (Option<String>, GlympseAttempt) {
    let url = build_glympse_login_url();
    match http.get(&url).send().await {
        Ok(response) => {
            let status = response.status();
            let content_type = response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .map(ToString::to_string);
            match response.text().await {
                Ok(text) => {
                    let token = serde_json::from_str::<Value>(&text).ok().and_then(|value| {
                        value
                            .pointer("/response/access_token")
                            .and_then(Value::as_str)
                            .map(ToString::to_string)
                    });
                    let parsed = token.is_some();
                    (
                        token,
                        GlympseAttempt {
                            url: redact_url(&url),
                            status: Some(status.as_u16()),
                            ok: status.is_success(),
                            content_type,
                            parsed,
                            message: if parsed {
                                "Obtained a Glympse anonymous viewer token".to_string()
                            } else {
                                "Viewer login response did not contain an access token".to_string()
                            },
                            response_preview: Some(response_preview(&text)),
                        },
                    )
                }
                Err(error) => (
                    None,
                    GlympseAttempt {
                        url: redact_url(&url),
                        status: Some(status.as_u16()),
                        ok: status.is_success(),
                        content_type,
                        parsed: false,
                        message: format!("Failed reading response body: {error}"),
                        response_preview: None,
                    },
                ),
            }
        }
        Err(error) => (
            None,
            GlympseAttempt {
                url: redact_url(&url),
                status: None,
                ok: false,
                content_type: None,
                parsed: false,
                message: error.to_string(),
                response_preview: None,
            },
        ),
    }
}

fn response_preview(text: &str) -> String {
    redact_text(text)
        .chars()
        .filter(|ch| !ch.is_control() || *ch == '\n' || *ch == '\t')
        .take(600)
        .collect::<String>()
        .trim()
        .to_string()
}

fn redact_url(raw: &str) -> String {
    if let Ok(mut url) = Url::parse(raw) {
        let mut pairs = Vec::new();
        let mut redacted_any = false;
        for (key, value) in url.query_pairs() {
            if matches!(key.as_ref(), "oauth_token" | "password" | "api_key") {
                pairs.push((key.to_string(), "[redacted]".to_string()));
                redacted_any = true;
            } else {
                pairs.push((key.to_string(), value.to_string()));
            }
        }
        if redacted_any {
            url.query_pairs_mut().clear().extend_pairs(pairs);
            return url.to_string();
        }
    }

    redact_text(raw)
}

fn redact_text(text: &str) -> String {
    let with_access_token = Regex::new(r#""access_token"\s*:\s*"[^"]+""#)
        .map(|regex| {
            regex
                .replace_all(text, r#""access_token":"[redacted]""#)
                .to_string()
        })
        .unwrap_or_else(|_| text.to_string());
    Regex::new(r#"(?i)(oauth_token|password|api_key)=([^&\s"]+)"#)
        .map(|regex| {
            regex
                .replace_all(&with_access_token, "$1=[redacted]")
                .to_string()
        })
        .unwrap_or(with_access_token)
}

fn extract_glympse_code(source: &str) -> Option<String> {
    let trimmed = source.trim();
    if trimmed.is_empty() {
        return None;
    }

    if let Ok(url) = Url::parse(trimmed) {
        for (key, value) in url.query_pairs() {
            let key = key.to_ascii_lowercase();
            if matches!(
                key.as_str(),
                "invite" | "code" | "g" | "id" | "ticket" | "share" | "link"
            ) && value.trim().len() >= 2
            {
                return Some(value.trim().to_string());
            }
        }

        if let Some(segments) = url.path_segments() {
            let ignored = [
                "app", "ext", "g", "glympse", "invite", "map", "share", "ticket",
            ];
            for segment in segments.collect::<Vec<_>>().into_iter().rev() {
                let decoded = urlencoding::decode(segment).unwrap_or_else(|_| segment.into());
                let cleaned = decoded.trim().trim_matches('/');
                if cleaned.len() >= 2 && !ignored.contains(&cleaned.to_ascii_lowercase().as_str()) {
                    return Some(cleaned.to_string());
                }
            }
        }
    }

    Some(trimmed.to_string())
}

fn parse_glympse_response(text: &str) -> Result<(LocationFix, Option<String>), String> {
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        let next_token = find_string_key(&value, "next");
        if let Some(location) = parse_ticket_invite_location(&value) {
            return Ok((location, next_token));
        }
        let mut candidates = Vec::new();
        collect_location_candidates(&value, "$", &mut candidates);
        if let Some(best) = pick_best_candidate(candidates) {
            return Ok((best.fix, next_token));
        }
        if let Some(message) = describe_glympse_response_without_location(&value) {
            return Err(message);
        }
    }

    if let Some(location) = scan_text_for_location(text) {
        return Ok((location, None));
    }

    Err("Response did not contain recognizable lat/lng coordinates".to_string())
}

fn apply_source_label_hint(location: &mut LocationFix, label_hint: Option<&str>) {
    let Some(label_hint) = label_hint
        .map(str::trim)
        .filter(|label_hint| !label_hint.is_empty())
    else {
        return;
    };

    if !location
        .source_label
        .as_deref()
        .is_some_and(is_usable_glympse_identity)
    {
        location.source_label = Some(label_hint.to_string());
    }
}

fn describe_glympse_response_without_location(value: &Value) -> Option<String> {
    let meta_error = value
        .pointer("/meta/error")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty());

    let response = value.get("response").or_else(|| value.get("result"));
    if let Some(response) = response.and_then(Value::as_object) {
        if response
            .get("type")
            .and_then(Value::as_str)
            .is_some_and(|value| value.eq_ignore_ascii_case("group"))
        {
            let tag = response
                .get("name")
                .and_then(Value::as_str)
                .or_else(|| response.get("id").and_then(Value::as_str))
                .unwrap_or("this tag");
            return Some(format!(
                "Glympse tag {tag} loaded, but it currently has no active shared positions"
            ));
        }
    }

    match meta_error {
        Some("invite_code") => Some("Glympse did not recognize this invite code".to_string()),
        Some("unexpected_error") => {
            Some("Glympse returned unexpected_error for this invite endpoint".to_string())
        }
        Some(error) => Some(format!("Glympse returned {error}")),
        None => None,
    }
}

fn parse_ticket_invite_location(value: &Value) -> Option<LocationFix> {
    let response = value.get("response").or_else(|| value.get("result"))?;
    let location_stream = response.get("location")?.as_array()?;
    let compressed = !response
        .get("uncompressed")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let source_label = ticket_invite_label(response);

    let mut previous: Option<Vec<Option<f64>>> = None;
    let mut latest = None;
    for row in location_stream {
        let mut item = row
            .as_array()?
            .iter()
            .map(value_as_f64)
            .collect::<Vec<Option<f64>>>();
        if compressed {
            if let Some(prev) = &previous {
                for index in 0..=7 {
                    if let Some(Some(current)) = item.get_mut(index) {
                        if let Some(previous_value) = prev.get(index).and_then(|value| *value) {
                            *current += previous_value;
                        }
                    }
                }
            }
        }
        previous = Some(item.clone());

        let timestamp_ms = item
            .first()
            .and_then(|value| value.and_then(timestamp_to_ms));
        let lat = item
            .get(1)
            .and_then(|value| value.and_then(|number| normalize_coordinate(number, true)))?;
        let lng = item
            .get(2)
            .and_then(|value| value.and_then(|number| normalize_coordinate(number, false)))?;

        latest = Some(LocationFix {
            lat,
            lng,
            accuracy: item.get(6).and_then(|value| *value),
            altitude: item.get(5).and_then(|value| *value),
            speed: item
                .get(3)
                .and_then(|value| *value)
                .map(|speed| speed * 0.01),
            heading: item.get(4).and_then(|value| *value),
            timestamp_ms,
            source_label: source_label
                .clone()
                .or_else(|| Some("Glympse invite".to_string())),
        });
    }

    latest
}

fn ticket_invite_label(response: &Value) -> Option<String> {
    let data = response
        .get("properties")
        .or_else(|| response.get("data"))?
        .as_array()?;
    for wanted in ["name", "owner", "app"] {
        for item in data {
            let Some(name) = item.get("n").and_then(Value::as_str) else {
                continue;
            };
            if name == wanted {
                let Some(value) = item.get("v") else {
                    continue;
                };
                if let Some(text) = value.as_str().filter(|text| !text.trim().is_empty()) {
                    return Some(text.to_string());
                }
                if value.is_object() || value.is_array() {
                    return Some(value.to_string());
                }
            }
        }
    }
    None
}

#[derive(Debug, Clone)]
struct LocationCandidate {
    fix: LocationFix,
    score: i64,
}

fn collect_location_candidates(value: &Value, path: &str, out: &mut Vec<LocationCandidate>) {
    match value {
        Value::Object(map) => {
            if let Some(fix) = location_from_object(map, path) {
                let score = candidate_score(&fix, path);
                out.push(LocationCandidate { fix, score });
            }

            for (key, child) in map {
                collect_location_candidates(child, &format!("{path}.{key}"), out);
            }
        }
        Value::Array(items) => {
            if let Some(fix) = location_from_array(items, path) {
                let score = candidate_score(&fix, path) - 4;
                out.push(LocationCandidate { fix, score });
            }

            for (index, child) in items.iter().enumerate() {
                collect_location_candidates(child, &format!("{path}[{index}]"), out);
            }
        }
        _ => {}
    }
}

fn location_from_object(map: &serde_json::Map<String, Value>, path: &str) -> Option<LocationFix> {
    let lat = first_coordinate(map, true, &["lat", "latitude", "latitudeE6", "latitudeE7"])?;
    let lng = first_coordinate(
        map,
        false,
        &[
            "lng",
            "lon",
            "long",
            "longitude",
            "longitudeE6",
            "longitudeE7",
        ],
    )?;

    Some(LocationFix {
        lat,
        lng,
        accuracy: first_number(map, &["accuracy", "horizontalAccuracy", "hacc"]),
        altitude: first_number(map, &["alt", "altitude", "elevation"]),
        speed: first_number(map, &["speed", "velocity"]),
        heading: first_number(map, &["heading", "bearing", "course"]),
        timestamp_ms: first_timestamp(map),
        source_label: Some(path.to_string()),
    })
}

fn location_from_array(items: &[Value], path: &str) -> Option<LocationFix> {
    if items.len() < 2 {
        return None;
    }
    let first = value_as_f64(&items[0])?;
    let second = value_as_f64(&items[1])?;

    let direct_lat_lng = normalize_coordinate(first, true)
        .zip(normalize_coordinate(second, false))
        .map(|(lat, lng)| LocationFix {
            lat,
            lng,
            accuracy: None,
            altitude: None,
            speed: None,
            heading: None,
            timestamp_ms: None,
            source_label: Some(path.to_string()),
        });

    if direct_lat_lng.is_some() && path.to_ascii_lowercase().contains("lat") {
        return direct_lat_lng;
    }

    normalize_coordinate(second, true)
        .zip(normalize_coordinate(first, false))
        .map(|(lat, lng)| LocationFix {
            lat,
            lng,
            accuracy: None,
            altitude: None,
            speed: None,
            heading: None,
            timestamp_ms: None,
            source_label: Some(path.to_string()),
        })
        .or(direct_lat_lng)
}

fn pick_best_candidate(candidates: Vec<LocationCandidate>) -> Option<LocationCandidate> {
    candidates.into_iter().max_by_key(|candidate| {
        (
            candidate.fix.timestamp_ms.unwrap_or(0),
            candidate.score,
            candidate
                .fix
                .accuracy
                .map(|value| -(value.round() as i64))
                .unwrap_or(0),
        )
    })
}

fn candidate_score(fix: &LocationFix, path: &str) -> i64 {
    let lower = path.to_ascii_lowercase();
    let mut score = 0;
    for token in ["current", "location", "locations", "position", "last"] {
        if lower.contains(token) {
            score += 8;
        }
    }
    if lower.contains("history") || lower.contains("route") || lower.contains("path") {
        score -= 4;
    }
    if fix.timestamp_ms.is_some() {
        score += 4;
    }
    if fix.accuracy.is_some() {
        score += 2;
    }
    score
}

fn first_coordinate(
    map: &serde_json::Map<String, Value>,
    is_lat: bool,
    keys: &[&str],
) -> Option<f64> {
    for requested in keys {
        for (key, value) in map {
            if key.eq_ignore_ascii_case(requested) {
                if let Some(number) =
                    value_as_f64(value).and_then(|n| normalize_coordinate(n, is_lat))
                {
                    return Some(number);
                }
            }
        }
    }
    None
}

fn first_number(map: &serde_json::Map<String, Value>, keys: &[&str]) -> Option<f64> {
    for requested in keys {
        for (key, value) in map {
            if key.eq_ignore_ascii_case(requested) {
                return value_as_f64(value);
            }
        }
    }
    None
}

fn first_timestamp(map: &serde_json::Map<String, Value>) -> Option<u64> {
    for requested in [
        "timestampMs",
        "timestamp",
        "time",
        "created",
        "createdAt",
        "updated",
        "updatedAt",
        "lastSeen",
    ] {
        for (key, value) in map {
            if key.eq_ignore_ascii_case(requested) {
                if let Some(timestamp) = value_as_f64(value).and_then(timestamp_to_ms) {
                    return Some(timestamp);
                }
            }
        }
    }
    None
}

fn find_string_key(value: &Value, requested: &str) -> Option<String> {
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                if key.eq_ignore_ascii_case(requested) {
                    if let Some(text) = child.as_str().filter(|text| !text.trim().is_empty()) {
                        return Some(text.to_string());
                    }
                }
                if let Some(found) = find_string_key(child, requested) {
                    return Some(found);
                }
            }
            None
        }
        Value::Array(items) => items
            .iter()
            .find_map(|item| find_string_key(item, requested)),
        _ => None,
    }
}

fn scan_text_for_location(text: &str) -> Option<LocationFix> {
    let lat_re =
        Regex::new(r#"(?i)["']?(lat|latitude)["']?\s*[:=]\s*["']?(-?\d+(?:\.\d+)?)"#).ok()?;
    let lng_re =
        Regex::new(r#"(?i)["']?(lng|lon|long|longitude)["']?\s*[:=]\s*["']?(-?\d+(?:\.\d+)?)"#)
            .ok()?;

    for lat_match in lat_re.captures_iter(text) {
        let lat_start = lat_match.get(0)?.start();
        let lat_raw = lat_match.get(2)?.as_str().parse::<f64>().ok()?;
        let window = safe_text_window(text, lat_start, 500);
        if let Some(lng_match) = lng_re.captures(window) {
            let lng_raw = lng_match.get(2)?.as_str().parse::<f64>().ok()?;
            if let Some((lat, lng)) =
                normalize_coordinate(lat_raw, true).zip(normalize_coordinate(lng_raw, false))
            {
                return Some(LocationFix {
                    lat,
                    lng,
                    accuracy: None,
                    altitude: None,
                    speed: None,
                    heading: None,
                    timestamp_ms: None,
                    source_label: Some("embedded text".to_string()),
                });
            }
        }
    }

    None
}

fn safe_text_window(text: &str, center: usize, radius: usize) -> &str {
    let start_floor = center.saturating_sub(radius);
    let end_ceiling = (center + radius).min(text.len());
    let start = previous_char_boundary(text, start_floor);
    let end = next_char_boundary(text, end_ceiling);
    &text[start..end]
}

fn previous_char_boundary(text: &str, mut index: usize) -> usize {
    while index > 0 && !text.is_char_boundary(index) {
        index -= 1;
    }
    index
}

fn next_char_boundary(text: &str, mut index: usize) -> usize {
    while index < text.len() && !text.is_char_boundary(index) {
        index += 1;
    }
    index
}

fn value_as_f64(value: &Value) -> Option<f64> {
    match value {
        Value::Number(number) => number.as_f64(),
        Value::String(text) => text.trim().parse::<f64>().ok(),
        _ => None,
    }
}

fn normalize_coordinate(value: f64, is_lat: bool) -> Option<f64> {
    let max = if is_lat { 90.0 } else { 180.0 };
    if value.abs() <= max {
        return Some(value);
    }
    for divisor in [1_000_000.0, 10_000_000.0] {
        let scaled = value / divisor;
        if scaled.abs() <= max {
            return Some(scaled);
        }
    }
    None
}

fn timestamp_to_ms(value: f64) -> Option<u64> {
    if !value.is_finite() || value <= 0.0 {
        return None;
    }
    if value < 4_000_000_000.0 {
        Some((value * 1000.0).round() as u64)
    } else if value > 10_000_000_000_000.0 {
        Some((value / 1000.0).round() as u64)
    } else {
        Some(value.round() as u64)
    }
}

fn build_caltopo_url(
    settings: &BridgeSettings,
    location: &LocationFix,
    caltopo_id: &str,
) -> String {
    let key = urlencoding::encode(settings.caltopo_connect_key.trim());
    let id = urlencoding::encode(caltopo_id);
    let mut url = format!(
        "https://caltopo.com/api/v1/position/report/{key}?id={id}&lat={:.7}&lng={:.7}",
        location.lat, location.lng
    );

    if settings.include_altitude {
        if let Some(altitude) = location.altitude {
            url.push_str("&alt=");
            url.push_str(&format!("{altitude:.1}"));
        }
    }

    url
}

fn caltopo_id_for_location(location: &LocationFix) -> Result<String, String> {
    location
        .source_label
        .as_deref()
        .filter(|value| is_usable_glympse_identity(value))
        .map(normalize_caltopo_device_id)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| {
            "Not forwarded: Glympse did not provide a usable user or track name for this fix"
                .to_string()
        })
}

fn is_usable_glympse_identity(value: &str) -> bool {
    let trimmed = value.trim();
    if trimmed.starts_with('$') || normalize_caltopo_device_id(trimmed).is_empty() {
        return false;
    }

    !matches!(
        trimmed.to_ascii_lowercase().as_str(),
        "embedded text" | "glympse invite" | "unnamed glympse user"
    )
}

fn normalize_caltopo_device_id(value: &str) -> String {
    value
        .trim()
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect()
}

fn merge_location(locations: &mut Vec<LocationFix>, location: LocationFix) {
    let key = location_dedupe_key(&location);
    if let Some(existing) = locations
        .iter_mut()
        .find(|candidate| location_dedupe_key(candidate) == key)
    {
        let existing_ts = existing.timestamp_ms.unwrap_or(0);
        let new_ts = location.timestamp_ms.unwrap_or(0);
        if new_ts >= existing_ts {
            *existing = location;
        }
        return;
    }

    locations.push(location);
}

fn location_dedupe_key(location: &LocationFix) -> String {
    location
        .source_label
        .as_deref()
        .filter(|value| is_usable_glympse_identity(value))
        .map(normalize_caltopo_device_id)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| format!("{:.6}:{:.6}", location.lat, location.lng))
}

fn location_signature(location: &LocationFix) -> String {
    format!(
        "{:.6}:{:.6}:{}",
        location.lat,
        location.lng,
        location.timestamp_ms.unwrap_or(0)
    )
}

fn emit_outcome(app: &AppHandle, outcome: &PollOutcome) {
    let _ = app.emit("locations-updated", outcome.locations.clone());
    if let Some(location) = &outcome.location {
        let _ = app.emit("location-updated", location.clone());
    }
    for forward in &outcome.forwards {
        let _ = app.emit("forward-result", forward.clone());
    }
}

fn emit_status(app: &AppHandle, running: bool, message: impl Into<String>) {
    let _ = app.emit(
        "bridge-status",
        BridgeStatus {
            running,
            message: message.into(),
        },
    );
}

fn emit_log(app: &AppHandle, level: &str, message: impl Into<String>) {
    let _ = app.emit(
        "bridge-log",
        BridgeLog {
            level: level.to_string(),
            message: message.into(),
            timestamp_ms: now_ms(),
        },
    );
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(AppState::new())
        .invoke_handler(tauri::generate_handler![
            diagnose_glympse,
            get_status,
            poll_once,
            start_bridge,
            stop_bridge,
            test_glympse
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_settings() -> BridgeSettings {
        BridgeSettings {
            glympse_source: "https://glympse.com/!ABC123".to_string(),
            caltopo_connect_key: "Sequoia".to_string(),
            poll_interval_secs: 5,
            forward_unchanged: false,
            include_altitude: true,
        }
    }

    #[test]
    fn extracts_codes_from_urls_and_raw_codes() {
        assert_eq!(
            extract_glympse_code("https://glympse.com/!ABC123").as_deref(),
            Some("!ABC123")
        );
        assert_eq!(
            extract_glympse_code("https://example.com/share?invite=XYZ789").as_deref(),
            Some("XYZ789")
        );
        assert_eq!(extract_glympse_code("RAWCODE").as_deref(), Some("RAWCODE"));
    }

    #[test]
    fn builds_glympse_urls_for_bang_and_plain_invites() {
        let urls = build_glympse_request_urls("https://glympse.com/!ABC123", None, Some("TOKEN"));
        assert!(!urls.iter().any(|url| url.contains("oauth_token=")));
        assert!(urls.iter().any(|url| url.contains("invites/%21ABC123")));
        assert!(urls.iter().any(|url| url.contains("invites/ABC123")));
        assert!(urls
            .iter()
            .any(|url| url.contains("groups/ABC123?branding=true")));
        assert!(urls.iter().any(|url| url == "https://glympse.com/!ABC123"));
    }

    #[test]
    fn builds_glympse_urls_with_delta_token() {
        let urls = build_glympse_request_urls("ABC123", Some("NEXT VALUE"), Some("TOKEN"));
        assert!(urls.iter().any(|url| url.contains("next=NEXT%20VALUE")));
    }

    #[test]
    fn redacts_diagnostic_tokens() {
        let url = "https://api.glympse.com/v2/invites/ABC?oauth_token=secret&api_key=key&next=1";
        assert_eq!(
            redact_url(url),
            "https://api.glympse.com/v2/invites/ABC?oauth_token=%5Bredacted%5D&api_key=%5Bredacted%5D&next=1"
        );
        assert!(!response_preview(r#"{"access_token":"secret"}"#).contains("secret"));
    }

    #[test]
    fn normalizes_direct_e6_and_e7_coordinates() {
        assert_eq!(normalize_coordinate(37.123, true), Some(37.123));
        assert_eq!(normalize_coordinate(37_123_456.0, true), Some(37.123456));
        assert_eq!(
            normalize_coordinate(-1_223_456_789.0, false),
            Some(-122.3456789)
        );
    }

    #[test]
    fn parses_nested_json_location() {
        let text = r#"{
          "result": {
            "invite": {"next": "abc"},
            "user": {
              "location": {
                "latitudeE7": 364737500,
                "longitudeE7": -1188530200,
                "accuracy": 12,
                "timestamp": 1779500000
              }
            }
          }
        }"#;

        let (fix, next) = parse_glympse_response(text).expect("location");
        assert_eq!(next.as_deref(), Some("abc"));
        assert!((fix.lat - 36.47375).abs() < 0.000001);
        assert!((fix.lng + 118.85302).abs() < 0.000001);
        assert_eq!(fix.accuracy, Some(12.0));
        assert_eq!(fix.timestamp_ms, Some(1_779_500_000_000));
    }

    #[test]
    fn parses_uncompressed_glympse_ticket_invite_stream() {
        let text = r#"{
          "result": "ok",
          "response": {
            "type": "ticket_invite",
            "uncompressed": true,
            "data": [{"t": 1779500000000, "n": "name", "v": "Sequoia test"}],
            "location": [
              [1779500000000, 36473750, -118853020, 122, 90, 1000, 8, 4],
              [1779500005000, 36473850, -118853120, 134, 95, 1002, 6, 3]
            ]
          }
        }"#;

        let (fix, _) = parse_glympse_response(text).expect("ticket location");
        assert!((fix.lat - 36.47385).abs() < 0.000001);
        assert!((fix.lng + 118.85312).abs() < 0.000001);
        assert_eq!(fix.speed, Some(1.34));
        assert_eq!(fix.heading, Some(95.0));
        assert_eq!(fix.altitude, Some(1002.0));
        assert_eq!(fix.accuracy, Some(6.0));
        assert_eq!(fix.timestamp_ms, Some(1_779_500_005_000));
        assert_eq!(fix.source_label.as_deref(), Some("Sequoia test"));
    }

    #[test]
    fn parses_ticket_invite_name_from_properties() {
        let text = r#"{
          "result": "ok",
          "response": {
            "type": "ticket_invite",
            "uncompressed": true,
            "properties": [{"t": 1779500000000, "n": "name", "v": "Ben Ko6cnt"}],
            "location": [
              [1779500000000, 37345001, -121960338, 0, 0, 31, 8, 0]
            ]
          }
        }"#;

        let (fix, _) = parse_glympse_response(text).expect("ticket location");
        assert_eq!(fix.source_label.as_deref(), Some("Ben Ko6cnt"));
    }

    #[test]
    fn parses_ticket_invite_name_after_unrelated_properties() {
        let text = r#"{
          "result": "ok",
          "response": {
            "type": "ticket_invite",
            "uncompressed": true,
            "properties": [
              {"n": "battery", "v": 92},
              {"x": "ignored"},
              {"n": "name", "v": "Lead Vehicle"}
            ],
            "location": [
              [1779500000000, 37345001, -121960338, 0, 0, 31, 8, 0]
            ]
          }
        }"#;

        let (fix, _) = parse_glympse_response(text).expect("ticket location");
        assert_eq!(fix.source_label.as_deref(), Some("Lead Vehicle"));
    }

    #[test]
    fn parses_compressed_glympse_ticket_invite_stream() {
        let text = r#"{
          "result": "ok",
          "response": {
            "type": "ticket_invite",
            "data": [],
            "location": [
              [1779500000000, 36473750, -118853020, 122, 90, 1000, 8, 4],
              [5000, 100, -100, 12, 5, 2, -2, -1]
            ]
          }
        }"#;

        let (fix, _) = parse_glympse_response(text).expect("compressed ticket location");
        assert!((fix.lat - 36.47385).abs() < 0.000001);
        assert!((fix.lng + 118.85312).abs() < 0.000001);
        assert_eq!(fix.speed, Some(1.34));
        assert_eq!(fix.heading, Some(95.0));
        assert_eq!(fix.altitude, Some(1002.0));
        assert_eq!(fix.accuracy, Some(6.0));
        assert_eq!(fix.timestamp_ms, Some(1_779_500_005_000));
    }

    #[test]
    fn scans_embedded_text_coordinates() {
        let text = r#"<script>window.bootstrap = { lat: "36.47375", lng: "-118.85302" };</script>"#;
        let (fix, _) = parse_glympse_response(text).expect("embedded location");
        assert!((fix.lat - 36.47375).abs() < 0.000001);
        assert!((fix.lng + 118.85302).abs() < 0.000001);
    }

    #[test]
    fn scans_embedded_text_coordinates_with_multibyte_prefix() {
        let text = format!(
            "a{}<script>window.bootstrap = {{ lat: \"36.47375\", lng: \"-118.85302\" }};</script>",
            "é".repeat(500)
        );
        let (fix, _) = parse_glympse_response(&text).expect("embedded location");
        assert!((fix.lat - 36.47375).abs() < 0.000001);
        assert!((fix.lng + 118.85302).abs() < 0.000001);
    }

    #[test]
    fn explains_public_tag_without_active_positions() {
        let text = r#"{
          "result": "ok",
          "response": {
            "type": "group",
            "id": "639712",
            "name": "sequoia2026",
            "branding": {
              "branding": {
                "definedRoutes": [
                  {"name": "Route", "path": "https://example.test/route"}
                ]
              }
            }
          }
        }"#;

        let error = parse_glympse_response(text).expect_err("tag has no active location");
        assert!(error.contains("sequoia2026"));
        assert!(error.contains("no active shared positions"));
    }

    #[test]
    fn extracts_member_invites_from_public_group_response() {
        let text = r#"{
          "result": "ok",
          "response": {
            "type": "group",
            "members": [
              {"invite": "FIRST123", "nickname": "Lead"},
              {"invite": "SECOND456", "nickname": "Sweep"},
              {"invite": "FIRST123", "nickname": "Duplicate"}
            ]
          }
        }"#;

        assert_eq!(
            extract_group_invites(text),
            vec![
                GlympseInvite {
                    code: "FIRST123".to_string(),
                    label: Some("Lead".to_string()),
                },
                GlympseInvite {
                    code: "SECOND456".to_string(),
                    label: Some("Sweep".to_string()),
                },
            ]
        );
    }

    #[test]
    fn extracts_invites_from_public_group_event_response() {
        let text = r#"{
          "result": "ok",
          "response": {
            "items": [
              {"type": "invite", "invite": "ACTIVE123"},
              {"type": "swap", "invite": "SWAPPED456"},
              {"type": "delete", "invite": "OLD789"}
            ]
          }
        }"#;

        assert_eq!(
            extract_group_invites(text),
            vec![
                GlympseInvite {
                    code: "ACTIVE123".to_string(),
                    label: None,
                },
                GlympseInvite {
                    code: "SWAPPED456".to_string(),
                    label: None,
                },
            ]
        );
    }

    #[test]
    fn builds_one_authenticated_member_invite_url_for_group_members() {
        let urls = build_glympse_member_request_urls("MEMBER123", Some("TOKEN"));
        assert_eq!(
            urls,
            vec!["https://api.glympse.com/v2/invites/MEMBER123?next=0&debug=true".to_string()]
        );
    }

    #[test]
    fn applies_group_member_label_hint_to_generic_member_location() {
        let mut location = LocationFix {
            lat: 37.345005,
            lng: -121.960396,
            accuracy: None,
            altitude: None,
            speed: None,
            heading: None,
            timestamp_ms: None,
            source_label: Some("$.response.location".to_string()),
        };

        apply_source_label_hint(&mut location, Some("Lead One"));

        assert_eq!(location.source_label.as_deref(), Some("Lead One"));
        assert_eq!(caltopo_id_for_location(&location).expect("id"), "LeadOne");
    }

    #[test]
    fn preserves_real_glympse_name_over_group_label_hint() {
        let mut location = LocationFix {
            lat: 37.345005,
            lng: -121.960396,
            accuracy: None,
            altitude: None,
            speed: None,
            heading: None,
            timestamp_ms: None,
            source_label: Some("Trail Medic".to_string()),
        };

        apply_source_label_hint(&mut location, Some("Member Card Label"));

        assert_eq!(location.source_label.as_deref(), Some("Trail Medic"));
        assert_eq!(
            caltopo_id_for_location(&location).expect("id"),
            "TrailMedic"
        );
    }

    #[test]
    fn builds_caltopo_url_with_glympse_name_id() {
        let settings = base_settings();
        let location = LocationFix {
            lat: 36.47375,
            lng: -118.85302,
            accuracy: None,
            altitude: Some(1250.4),
            speed: None,
            heading: None,
            timestamp_ms: None,
            source_label: Some("Car 1".to_string()),
        };
        let caltopo_id = caltopo_id_for_location(&location).expect("id");
        let url = build_caltopo_url(&settings, &location, &caltopo_id);
        assert!(url.contains("/position/report/Sequoia?"));
        assert!(url.contains("id=Car1"));
        assert!(url.contains("lat=36.4737500"));
        assert!(url.contains("lng=-118.8530200"));
        assert!(url.contains("alt=1250.4"));
    }

    #[test]
    fn derives_caltopo_id_from_glympse_name() {
        let settings = base_settings();
        let location = LocationFix {
            lat: 37.345005,
            lng: -121.960396,
            accuracy: None,
            altitude: None,
            speed: None,
            heading: None,
            timestamp_ms: None,
            source_label: Some("Ben Ko6cnt".to_string()),
        };
        let caltopo_id = caltopo_id_for_location(&location).expect("id");
        assert_eq!(caltopo_id, "BenKo6cnt");

        let url = build_caltopo_url(&settings, &location, &caltopo_id);
        assert!(url.contains("/position/report/Sequoia?id=BenKo6cnt&lat="));
    }

    #[test]
    fn rejects_generic_location_labels_instead_of_inventing_an_id() {
        let location = LocationFix {
            lat: 37.345005,
            lng: -121.960396,
            accuracy: None,
            altitude: None,
            speed: None,
            heading: None,
            timestamp_ms: None,
            source_label: Some("$.response.location".to_string()),
        };
        let error = caltopo_id_for_location(&location).expect_err("generic label rejected");
        assert!(error.contains("Not forwarded"));
    }

    #[test]
    fn rejects_embedded_text_label_instead_of_using_default_id() {
        let location = LocationFix {
            lat: 37.345005,
            lng: -121.960396,
            accuracy: None,
            altitude: None,
            speed: None,
            heading: None,
            timestamp_ms: None,
            source_label: Some("embedded text".to_string()),
        };
        let error = caltopo_id_for_location(&location).expect_err("embedded label rejected");
        assert!(error.contains("Not forwarded"));
    }

    #[test]
    fn removes_hyphens_and_spaces_from_caltopo_device_id() {
        assert_eq!(normalize_caltopo_device_id("BBEV-uqvl"), "BBEVuqvl");
        assert_eq!(normalize_caltopo_device_id("Ben Ko6cnt"), "BenKo6cnt");
    }

    #[test]
    fn keeps_distinct_glympse_users_for_multi_member_forwarding() {
        let mut locations = Vec::new();
        merge_location(
            &mut locations,
            LocationFix {
                lat: 37.1,
                lng: -121.1,
                accuracy: None,
                altitude: None,
                speed: None,
                heading: None,
                timestamp_ms: Some(10),
                source_label: Some("Lead One".to_string()),
            },
        );
        merge_location(
            &mut locations,
            LocationFix {
                lat: 37.2,
                lng: -121.2,
                accuracy: None,
                altitude: None,
                speed: None,
                heading: None,
                timestamp_ms: Some(10),
                source_label: Some("Sweep Two".to_string()),
            },
        );
        merge_location(
            &mut locations,
            LocationFix {
                lat: 37.3,
                lng: -121.3,
                accuracy: None,
                altitude: None,
                speed: None,
                heading: None,
                timestamp_ms: Some(11),
                source_label: Some("Lead One".to_string()),
            },
        );

        assert_eq!(locations.len(), 2);
        assert!(locations.iter().any(|location| location
            .source_label
            .as_deref()
            .is_some_and(|label| label == "Lead One")
            && (location.lat - 37.3).abs() < f64::EPSILON));
        assert!(locations.iter().any(|location| location
            .source_label
            .as_deref()
            .is_some_and(|label| label == "Sweep Two")));
    }

    #[test]
    fn unchanged_signature_includes_position_and_timestamp() {
        let fix = LocationFix {
            lat: 36.4737501,
            lng: -118.8530201,
            accuracy: None,
            altitude: None,
            speed: None,
            heading: None,
            timestamp_ms: Some(10),
            source_label: None,
        };
        assert_eq!(location_signature(&fix), "36.473750:-118.853020:10");
    }

    #[tokio::test]
    #[ignore = "hits the live Glympse API and requires the Sequoia2026 public tag to be reachable"]
    async fn live_probe_sequoia2026_glympse_tag() {
        let http = Client::builder()
            .timeout(Duration::from_secs(15))
            .user_agent("Glympse CalTopo Bridge live probe/0.1")
            .build()
            .expect("reqwest client should build");
        let mut settings = base_settings();
        settings.glympse_source = "https://glympse.com/!Sequoia2026".to_string();

        let mut context = PollContext::default();
        let fetch = fetch_glympse_locations(&http, &settings, &mut context)
            .await
            .expect("active Glympse member locations");
        for location in &fetch.locations {
            println!(
                "{} {} -> {:?}",
                location.lat, location.lng, location.source_label
            );
        }

        assert!(!fetch.locations.is_empty());
    }
}
