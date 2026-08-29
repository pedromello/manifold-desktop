use reqwest::{Client, Response, StatusCode};
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

mod butler;
mod installer;
pub mod publisher;

const ENVIRONMENTS: [&str; 3] = ["development", "staging", "production"];
const API_PATH: &str = "/api/v1";
const DEVELOPMENT_API_ORIGIN: &str = "http://localhost:3000";
const PRODUCTION_API_ORIGIN: &str = "https://manifoldpowered.com";
const STORE_PAGE_SIZE: u8 = 12;
const LIBRARY_PAGE_SIZE: u8 = 100;
const CREDENTIAL_SERVICE: &str = "com.manifoldpowered.desktop";
const CREDENTIAL_USER: &str = "session";

pub(crate) struct ApiState {
    client: Mutex<Client>,
}

impl ApiState {
    fn new() -> Result<Self, String> {
        Ok(Self {
            client: Mutex::new(build_api_client()?),
        })
    }

    pub(crate) fn client(&self) -> Result<Client, String> {
        self.client
            .lock()
            .map(|client| client.clone())
            .map_err(|_| "the Manifold API client is unavailable".into())
    }

    fn clear_session(&self) -> Result<(), String> {
        let mut client = self
            .client
            .lock()
            .map_err(|_| "the Manifold API client is unavailable".to_string())?;
        delete_session_token();
        *client = build_api_client()?;
        Ok(())
    }
}

fn session_credential() -> Result<keyring::Entry, String> {
    keyring::Entry::new(CREDENTIAL_SERVICE, CREDENTIAL_USER)
        .map_err(|error| format!("could not access the system credential store: {error}"))
}

fn load_session_token() -> Option<String> {
    session_credential().ok()?.get_password().ok()
}

fn save_session_token(token: &str) -> Result<(), String> {
    session_credential()?
        .set_password(token)
        .map_err(|error| format!("could not save the session securely: {error}"))
}

fn delete_session_token() {
    if let Ok(credential) = session_credential() {
        let _ = credential.delete_credential();
    }
}

fn build_api_client() -> Result<Client, String> {
    let cookie_jar = Arc::new(reqwest::cookie::Jar::default());
    if let Some(token) = load_session_token() {
        cookie_jar.add_cookie_str(
            &format!("session_id={token}; Path=/; Secure; HttpOnly"),
            &url::Url::parse(PRODUCTION_API_ORIGIN)
                .map_err(|error| format!("invalid production API origin: {error}"))?,
        );
    }
    Client::builder()
        .cookie_provider(cookie_jar)
        .timeout(Duration::from_secs(15))
        .cookie_store(true)
        .user_agent(concat!("Manifold-Desktop/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|error| format!("failed to initialize the Manifold API client: {error}"))
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationInfo {
    version: &'static str,
    environment: String,
    api_base_url: String,
    platform: &'static str,
    architecture: &'static str,
}

#[derive(Debug, Deserialize)]
struct ApiError {
    message: Option<String>,
    action: Option<String>,
}

#[derive(Debug, Deserialize)]
struct DesktopApiErrorEnvelope {
    error: DesktopApiErrorBody,
}

#[derive(Debug, Deserialize)]
struct DesktopApiErrorBody {
    code: String,
    message: String,
    retryable: bool,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct DistributionError {
    code: String,
    message: String,
    retryable: bool,
}

impl DistributionError {
    fn service_unavailable(message: impl Into<String>) -> Self {
        Self {
            code: "SERVICE_UNAVAILABLE".into(),
            message: message.into(),
            retryable: true,
        }
    }

    fn invalid_request(message: impl Into<String>) -> Self {
        Self {
            code: "INVALID_REQUEST".into(),
            message: message.into(),
            retryable: false,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthUser {
    id: String,
    username: String,
    email: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthMessage {
    message: String,
}

#[derive(Debug, Deserialize)]
struct SessionApiResponse {
    token: String,
}

#[derive(Debug, Deserialize)]
struct GamesApiResponse {
    games: Vec<GamesApiGame>,
    pagination: GamesApiPagination,
    currency: String,
}

#[derive(Debug, Deserialize)]
struct GamesApiGame {
    id: String,
    slug: String,
    title: String,
    description: String,
    price: Option<String>,
    developer_name: String,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    media: GamesApiMedia,
    display_price: Option<GamesApiDisplayPrice>,
    discount_label: Option<String>,
    review_score: Option<String>,
    status: Option<String>,
    ownership_status: Option<String>,
    purchase_mode: Option<String>,
    external_offer: Option<GamesApiExternalOffer>,
}

#[derive(Debug, Deserialize)]
struct GamesApiExternalOffer {
    provider: String,
    amount: Option<String>,
    original_amount: Option<String>,
    discount_percent: Option<u32>,
    currency: Option<String>,
    url: String,
    captured_at: Option<String>,
}

#[derive(Debug, Default, Deserialize)]
struct GamesApiMedia {
    banner: Option<String>,
    icon: Option<String>,
}

#[derive(Debug, Deserialize)]
struct GamesApiDisplayPrice {
    amount: String,
    base_amount: Option<String>,
    currency: String,
    symbol: String,
}

#[derive(Debug, Deserialize)]
struct GamesApiPagination {
    page: u32,
    limit: u32,
    total: u32,
    pages: u32,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoreCatalog {
    games: Vec<StoreGame>,
    pagination: StorePagination,
    currency: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoreGame {
    id: String,
    slug: String,
    title: String,
    description: String,
    price: Option<String>,
    developer_name: String,
    tags: Vec<String>,
    banner_url: Option<String>,
    icon_url: Option<String>,
    display_price: Option<StoreDisplayPrice>,
    discount_label: Option<String>,
    review_score: Option<String>,
    #[serde(rename = "status")]
    status: String,
    #[serde(rename = "ownershipStatus")]
    ownership_status: String,
    #[serde(rename = "purchaseMode")]
    purchase_mode: String,
    #[serde(rename = "externalOffer")]
    external_offer: Option<StoreExternalOffer>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoreExternalOffer {
    provider: String,
    amount: Option<String>,
    original_amount: Option<String>,
    discount_percent: Option<u32>,
    currency: Option<String>,
    url: String,
    captured_at: Option<String>,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StoreDisplayPrice {
    amount: String,
    base_amount: Option<String>,
    currency: String,
    symbol: String,
}

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct StorePagination {
    page: u32,
    limit: u32,
    total: u32,
    pages: u32,
}

#[derive(Debug, Deserialize)]
struct LibraryApiResponse {
    games: Vec<LibraryApiItem>,
}

#[derive(Debug, Deserialize)]
struct LibraryApiItem {
    id: String,
    acquired_at: String,
    game: LibraryApiGame,
}

#[derive(Debug, Deserialize)]
struct LibraryApiGame {
    id: String,
    slug: String,
    title: String,
    description: String,
    developer_name: String,
    status: Option<String>,
    purchase_mode: Option<String>,
    #[serde(default)]
    media: GamesApiMedia,
}

#[derive(Debug, Deserialize)]
struct PurchasesApiResponse {
    purchases: Vec<PurchaseApiItem>,
}

#[derive(Debug, Deserialize)]
struct PurchaseApiItem {
    game_id: String,
    store_id: Option<String>,
    created_at: String,
    store_name: Option<String>,
    store_slug: Option<String>,
    store_logo_url: Option<String>,
}

#[derive(Debug, Deserialize)]
struct StoresApiResponse {
    stores: Vec<OutletApi>,
}

#[derive(Debug, Clone, Deserialize)]
struct OutletApi {
    id: String,
    slug: String,
    name: String,
    logo_url: Option<String>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryCatalog {
    games: Vec<LibraryGame>,
    total: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryGame {
    library_id: String,
    id: String,
    slug: String,
    title: String,
    description: String,
    developer_name: String,
    banner_url: Option<String>,
    icon_url: Option<String>,
    acquired_at: String,
    outlet: Option<LibraryOutlet>,
    acquisition_label: String,
    acquisition_type: String,
    status: String,
    purchase_mode: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct LibraryOutlet {
    id: String,
    slug: Option<String>,
    name: String,
    logo_url: Option<String>,
}

impl From<GamesApiResponse> for StoreCatalog {
    fn from(response: GamesApiResponse) -> Self {
        Self {
            games: response
                .games
                .into_iter()
                .map(|game| {
                    let status = normalized_game_status(game.status.as_deref());
                    let purchase_mode =
                        normalized_purchase_mode(game.purchase_mode.as_deref(), &status);
                    StoreGame {
                        id: game.id,
                        slug: game.slug,
                        title: game.title,
                        description: game.description,
                        price: game.price,
                        developer_name: game.developer_name,
                        tags: game.tags,
                        banner_url: game.media.banner,
                        icon_url: game.media.icon,
                        display_price: game.display_price.map(|price| StoreDisplayPrice {
                            amount: price.amount,
                            base_amount: price.base_amount,
                            currency: price.currency,
                            symbol: price.symbol,
                        }),
                        discount_label: game.discount_label,
                        review_score: game.review_score,
                        status,
                        ownership_status: match game.ownership_status.as_deref() {
                            Some("UNCLAIMED") => "UNCLAIMED".into(),
                            _ => "CLAIMED".into(),
                        },
                        purchase_mode,
                        external_offer: game.external_offer.and_then(StoreExternalOffer::from_api),
                    }
                })
                .collect(),
            pagination: StorePagination {
                page: response.pagination.page,
                limit: response.pagination.limit,
                total: response.pagination.total,
                pages: response.pagination.pages,
            },
            currency: response.currency,
        }
    }
}

fn validated_application_info(environment: String) -> Result<ApplicationInfo, String> {
    if !ENVIRONMENTS.contains(&environment.as_str()) {
        return Err(format!(
            "unsupported application environment: {environment}"
        ));
    }
    let staging_origin = std::env::var("MANIFOLD_API_BASE_URL").ok();
    let api_base_url = api_base_url(&environment, staging_origin.as_deref())?;
    let (platform, architecture) = desktop_target()?;
    Ok(ApplicationInfo {
        version: env!("CARGO_PKG_VERSION"),
        environment,
        api_base_url: api_base_url.to_string(),
        platform,
        architecture,
    })
}

fn desktop_target() -> Result<(&'static str, &'static str), String> {
    let platform = if cfg!(target_os = "windows") {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if cfg!(target_os = "linux") {
        "linux"
    } else {
        return Err("unsupported desktop platform".into());
    };
    let architecture = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        return Err("unsupported desktop architecture".into());
    };
    Ok((platform, architecture))
}

fn api_base_url(environment: &str, staging_origin: Option<&str>) -> Result<url::Url, String> {
    let origin = match environment {
        "development" => DEVELOPMENT_API_ORIGIN,
        "staging" => staging_origin.ok_or("staging requires MANIFOLD_API_BASE_URL")?,
        "production" => PRODUCTION_API_ORIGIN,
        other => return Err(format!("unsupported application environment: {other}")),
    };
    let origin = url::Url::parse(origin).map_err(|error| format!("invalid API origin: {error}"))?;
    if environment != "development" && origin.scheme() != "https" {
        return Err(format!("{environment} API origin must use HTTPS"));
    }
    origin
        .join(&format!("{API_PATH}/"))
        .map_err(|error| format!("invalid API URL: {error}"))
}

fn production_api_url(path: &str) -> Result<url::Url, String> {
    api_base_url("production", None)?
        .join(path)
        .map_err(|error| format!("invalid API URL: {error}"))
}

fn store_games_url(query: Option<&str>) -> Result<url::Url, String> {
    let query = query.map(str::trim).filter(|query| !query.is_empty());
    if query.is_some_and(|query| query.chars().count() > 80) {
        return Err("store search must contain at most 80 characters".into());
    }
    let mut url = production_api_url("games")?;
    {
        let mut pairs = url.query_pairs_mut();
        pairs.append_pair("limit", &STORE_PAGE_SIZE.to_string());
        if let Some(query) = query {
            pairs.append_pair("q", query);
        }
    }
    Ok(url)
}

fn normalized_game_status(status: Option<&str>) -> String {
    match status {
        Some("ONLY_DISPLAY") => "ONLY_DISPLAY",
        Some("INACTIVE") => "INACTIVE",
        Some("PRIVATE") => "PRIVATE",
        _ => "ACTIVE",
    }
    .into()
}

fn normalized_purchase_mode(mode: Option<&str>, status: &str) -> String {
    match mode {
        Some("STEAM_ONLY") => "STEAM_ONLY",
        Some("UNAVAILABLE") => "UNAVAILABLE",
        Some("PLATFORM") => "PLATFORM",
        _ if status == "ACTIVE" => "PLATFORM",
        _ => "UNAVAILABLE",
    }
    .into()
}

impl StoreExternalOffer {
    fn from_api(offer: GamesApiExternalOffer) -> Option<Self> {
        let url = url::Url::parse(&offer.url).ok()?;
        if url.scheme() != "https" || url.host_str() != Some("store.steampowered.com") {
            return None;
        }
        if offer.provider != "STEAM" {
            return None;
        }
        Some(Self {
            provider: offer.provider,
            amount: offer.amount,
            original_amount: offer.original_amount,
            discount_percent: offer.discount_percent,
            currency: offer.currency,
            url: offer.url,
            captured_at: offer.captured_at,
        })
    }
}

fn install_manifest_url(release_id: &str, artifact_id: &str) -> Result<url::Url, String> {
    let mut url = production_api_url(&format!("releases/{release_id}/manifest"))?;
    url.query_pairs_mut()
        .append_pair("schema_version", "1")
        .append_pair("artifact_id", artifact_id);
    Ok(url)
}

async fn api_json<T: DeserializeOwned>(response: Response) -> Result<T, String> {
    let status = response.status();
    if status.is_success() {
        return response
            .json::<T>()
            .await
            .map_err(|error| format!("Manifold API returned invalid data: {error}"));
    }
    let fallback = format!("Manifold API returned {status}");
    let error = response.json::<ApiError>().await.ok();
    let message = error
        .as_ref()
        .and_then(|body| body.message.clone())
        .unwrap_or(fallback);
    let action = error.and_then(|body| body.action);
    Err(match action {
        Some(action) => format!("{message}. {action}"),
        None => message,
    })
}

async fn distribution_api_json<T: DeserializeOwned>(
    response: Response,
) -> Result<T, DistributionError> {
    let status = response.status();
    if status.is_success() {
        return response.json::<T>().await.map_err(|error| {
            DistributionError::service_unavailable(format!(
                "Manifold API returned invalid distribution data: {error}"
            ))
        });
    }
    if let Ok(envelope) = response.json::<DesktopApiErrorEnvelope>().await {
        return Err(DistributionError {
            code: envelope.error.code,
            message: envelope.error.message,
            retryable: envelope.error.retryable,
        });
    }
    let (code, retryable) = match status {
        StatusCode::UNAUTHORIZED => ("AUTHENTICATION_REQUIRED", false),
        StatusCode::FORBIDDEN => ("ENTITLEMENT_REQUIRED", false),
        StatusCode::TOO_MANY_REQUESTS => ("RATE_LIMITED", true),
        status if status.is_server_error() => ("SERVICE_UNAVAILABLE", true),
        _ => ("INVALID_REQUEST", false),
    };
    Err(DistributionError {
        code: code.into(),
        message: format!("Manifold API returned {status}"),
        retryable,
    })
}

async fn send(_client: &Client, request: reqwest::RequestBuilder) -> Result<Response, String> {
    request
        .header(reqwest::header::ACCEPT, "application/json")
        .send()
        .await
        .map_err(|error| format!("could not reach the Manifold API: {error}"))
}

#[tauri::command]
fn application_info(environment: String) -> Result<ApplicationInfo, String> {
    validated_application_info(environment)
}

#[tauri::command]
async fn list_store_games(
    state: tauri::State<'_, ApiState>,
    query: Option<String>,
) -> Result<StoreCatalog, String> {
    let response = send(
        &state.client()?,
        state.client()?.get(store_games_url(query.as_deref())?),
    )
    .await?;
    Ok(api_json::<GamesApiResponse>(response).await?.into())
}

#[tauri::command]
async fn current_user(state: tauri::State<'_, ApiState>) -> Result<Option<AuthUser>, String> {
    let client = state.client()?;
    let response = send(&client, client.get(production_api_url("user")?)).await?;
    if matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        return Ok(None);
    }
    api_json(response).await.map(Some)
}

#[tauri::command]
async fn request_otp(
    state: tauri::State<'_, ApiState>,
    login: String,
) -> Result<AuthMessage, String> {
    let login = login.trim();
    if login.is_empty() || login.chars().count() > 255 {
        return Err("Enter a valid email or username".into());
    }
    let client = state.client()?;
    let response = send(
        &client,
        client
            .post(production_api_url("otp")?)
            .json(&serde_json::json!({ "login": login })),
    )
    .await?;
    let body: serde_json::Value = api_json(response).await?;
    Ok(AuthMessage {
        message: body["message"].as_str().unwrap_or("Code sent").to_string(),
    })
}

#[tauri::command]
async fn verify_otp(
    state: tauri::State<'_, ApiState>,
    login: String,
    code: String,
) -> Result<AuthUser, String> {
    let code = code.trim();
    if code.len() != 6 || !code.chars().all(|character| character.is_ascii_digit()) {
        return Err("Enter the 6-digit code from your email".into());
    }
    let client = state.client()?;
    let response = send(
        &client,
        client
            .post(production_api_url("otp/sessions")?)
            .json(&serde_json::json!({
                "login": login.trim(),
                "code": code,
            })),
    )
    .await?;
    let session: SessionApiResponse = api_json(response).await?;
    save_session_token(&session.token)?;
    let response = send(&client, client.get(production_api_url("user")?)).await?;
    api_json(response).await
}

#[tauri::command]
async fn create_account(
    state: tauri::State<'_, ApiState>,
    username: String,
    email: String,
) -> Result<AuthMessage, String> {
    let username = username.trim();
    let email = email.trim();
    if username.len() < 3
        || username.len() > 30
        || !username.chars().all(|c| c.is_ascii_alphanumeric())
    {
        return Err("Username must be 3 to 30 letters or numbers".into());
    }
    if !email.contains('@') || email.len() > 255 {
        return Err("Enter a valid email address".into());
    }
    let client = state.client()?;
    let response = send(
        &client,
        client
            .post(production_api_url("users")?)
            .json(&serde_json::json!({
                "username": username,
                "email": email,
                "password": null,
            })),
    )
    .await?;
    let _: serde_json::Value = api_json(response).await?;
    Ok(AuthMessage {
        message:
            "Account created. Activate it from the email we sent you, then sign in with a code."
                .into(),
    })
}

#[tauri::command]
async fn logout(state: tauri::State<'_, ApiState>) -> Result<(), String> {
    let client = state.client()?;
    let response = send(&client, client.delete(production_api_url("sessions")?)).await?;
    if !matches!(
        response.status(),
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
    ) {
        let _: serde_json::Value = api_json(response).await?;
    }
    state.clear_session()
}

#[tauri::command]
async fn list_library(state: tauri::State<'_, ApiState>) -> Result<LibraryCatalog, String> {
    let client = state.client()?;
    let library_url = format!("library?limit={LIBRARY_PAGE_SIZE}");
    let purchases_url = format!("user/purchases?limit={LIBRARY_PAGE_SIZE}");
    let stores_url = format!("public/stores?limit={LIBRARY_PAGE_SIZE}");

    let library: LibraryApiResponse =
        api_json(send(&client, client.get(production_api_url(&library_url)?)).await?).await?;
    let purchases: PurchasesApiResponse =
        api_json(send(&client, client.get(production_api_url(&purchases_url)?)).await?).await?;
    let stores: StoresApiResponse =
        api_json(send(&client, client.get(production_api_url(&stores_url)?)).await?).await?;

    let store_by_id: HashMap<String, OutletApi> = stores
        .stores
        .into_iter()
        .map(|store| (store.id.clone(), store))
        .collect();
    let mut purchase_by_game = HashMap::new();
    for purchase in purchases.purchases {
        purchase_by_game
            .entry(purchase.game_id.clone())
            .or_insert(purchase);
    }

    let games = library
        .games
        .into_iter()
        .map(|item| {
            let purchase = purchase_by_game.get(&item.game.id);
            let outlet = purchase.and_then(|purchase| {
                let id = purchase.store_id.clone()?;
                let fallback = store_by_id.get(&id);
                Some(LibraryOutlet {
                    id,
                    slug: purchase
                        .store_slug
                        .clone()
                        .or_else(|| fallback.map(|store| store.slug.clone())),
                    name: purchase
                        .store_name
                        .clone()
                        .or_else(|| fallback.map(|store| store.name.clone()))
                        .unwrap_or_else(|| "Partner Outlet".into()),
                    logo_url: purchase
                        .store_logo_url
                        .clone()
                        .or_else(|| fallback.and_then(|store| store.logo_url.clone())),
                })
            });
            let acquisition_label = match (purchase, outlet.as_ref()) {
                (_, Some(outlet)) => format!("Acquired via {}", outlet.name),
                (Some(_), None) => "Acquired via Manifold Store".into(),
                (None, None) => "Granted by Manifold".into(),
            };
            let acquisition_type = match (purchase, outlet.as_ref()) {
                (_, Some(_)) => "OUTLET",
                (Some(_), None) => "MANIFOLD_STORE",
                (None, None) => "GRANT",
            }
            .to_string();
            LibraryGame {
                library_id: item.id,
                id: item.game.id,
                slug: item.game.slug,
                title: item.game.title,
                description: item.game.description,
                developer_name: item.game.developer_name,
                banner_url: item.game.media.banner,
                icon_url: item.game.media.icon,
                acquired_at: purchase
                    .map(|p| p.created_at.clone())
                    .unwrap_or(item.acquired_at),
                outlet,
                acquisition_label,
                acquisition_type,
                status: normalized_game_status(item.game.status.as_deref()),
                purchase_mode: normalized_purchase_mode(
                    item.game.purchase_mode.as_deref(),
                    &normalized_game_status(item.game.status.as_deref()),
                ),
            }
        })
        .collect::<Vec<_>>();

    Ok(LibraryCatalog {
        total: games.len(),
        games,
    })
}

fn distribution_target() -> Result<(&'static str, &'static str), String> {
    let platform = if cfg!(target_os = "windows") {
        "WINDOWS"
    } else if cfg!(target_os = "macos") {
        "MAC"
    } else if cfg!(target_os = "linux") {
        "LINUX"
    } else {
        return Err("unsupported distribution platform".into());
    };
    let architecture = if cfg!(target_arch = "x86_64") {
        "X86_64"
    } else if cfg!(target_arch = "aarch64") {
        "AARCH64"
    } else {
        return Err("unsupported distribution architecture".into());
    };
    Ok((platform, architecture))
}

async fn latest_compatible_release(
    client: &Client,
    game_slug: &str,
) -> Result<installer::ReleaseSummary, DistributionError> {
    installer::validate_game_slug(game_slug).map_err(DistributionError::invalid_request)?;
    let (platform, architecture) =
        distribution_target().map_err(DistributionError::service_unavailable)?;
    let mut release_url = production_api_url(&format!("games/{game_slug}/releases/latest"))
        .map_err(DistributionError::service_unavailable)?;
    release_url
        .query_pairs_mut()
        .append_pair("platform", platform)
        .append_pair("arch", architecture);
    let response = send(client, client.get(release_url))
        .await
        .map_err(DistributionError::service_unavailable)?;
    distribution_api_json(response).await
}

#[tauri::command]
async fn resolve_latest_release(
    state: tauri::State<'_, ApiState>,
    game_slug: String,
) -> Result<installer::ReleaseSummary, DistributionError> {
    let client = state
        .client()
        .map_err(DistributionError::service_unavailable)?;
    latest_compatible_release(&client, &game_slug).await
}

#[tauri::command]
async fn resolve_install_plan(
    state: tauri::State<'_, ApiState>,
    game_slug: String,
) -> Result<installer::DistributionPlan, DistributionError> {
    let client = state
        .client()
        .map_err(DistributionError::service_unavailable)?;
    let release = latest_compatible_release(&client, &game_slug).await?;
    let manifest_url = install_manifest_url(&release.id, &release.artifact_id)
        .map_err(DistributionError::service_unavailable)?;
    let manifest: installer::InstallManifest = distribution_api_json(
        send(&client, client.get(manifest_url))
            .await
            .map_err(DistributionError::service_unavailable)?,
    )
    .await?;
    let download_url = production_api_url(&format!("artifacts/{}/download", release.artifact_id))
        .map_err(DistributionError::service_unavailable)?;
    let download: installer::DownloadAuthorization = distribution_api_json(
        send(&client, client.post(download_url))
            .await
            .map_err(DistributionError::service_unavailable)?,
    )
    .await?;
    Ok(installer::DistributionPlan {
        game_slug,
        release,
        manifest,
        download,
    })
}

fn validate_api_identifier(value: &str) -> Result<(), DistributionError> {
    let valid = value.len() == 36
        && value.chars().enumerate().all(|(index, character)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                character == '-'
            } else {
                character.is_ascii_hexdigit()
            }
        });
    if valid {
        Ok(())
    } else {
        Err(DistributionError::invalid_request("invalid API identifier"))
    }
}

fn update_plan_url(
    game_slug: &str,
    source_release_id: &str,
) -> Result<url::Url, DistributionError> {
    installer::validate_game_slug(game_slug).map_err(DistributionError::invalid_request)?;
    validate_api_identifier(source_release_id)?;
    let (platform, architecture) =
        distribution_target().map_err(DistributionError::service_unavailable)?;
    let mut url = production_api_url(&format!("games/{game_slug}/updates/latest"))
        .map_err(DistributionError::service_unavailable)?;
    url.query_pairs_mut()
        .append_pair("source_release_id", source_release_id)
        .append_pair("platform", platform)
        .append_pair("arch", architecture);
    Ok(url)
}

#[tauri::command]
async fn resolve_update_plan(
    state: tauri::State<'_, ApiState>,
    game_slug: String,
    source_release_id: String,
) -> Result<installer::UpdatePlan, DistributionError> {
    let client = state
        .client()
        .map_err(DistributionError::service_unavailable)?;
    let url = update_plan_url(&game_slug, &source_release_id)?;
    let response = send(&client, client.get(url))
        .await
        .map_err(DistributionError::service_unavailable)?;
    distribution_api_json(response).await
}

#[tauri::command]
async fn prepare_update_plan(
    state: tauri::State<'_, ApiState>,
    update: installer::UpdatePlan,
) -> Result<installer::UpdateExecutionPlan, DistributionError> {
    let client = state
        .client()
        .map_err(DistributionError::service_unavailable)?;
    let (source_id, target_id, target_artifact_id, fallback_artifact_id, patch_id) = match &update {
        installer::UpdatePlan::Patch {
            source,
            target,
            fallback_artifact_id,
            patch,
        } => (
            &source.id,
            &target.id,
            &target.artifact_id,
            fallback_artifact_id,
            Some(&patch.id),
        ),
        installer::UpdatePlan::Full {
            source,
            target,
            fallback_artifact_id,
            ..
        } => (
            &source.id,
            &target.id,
            &target.artifact_id,
            fallback_artifact_id,
            None,
        ),
    };
    for identifier in [
        source_id,
        target_id,
        target_artifact_id,
        fallback_artifact_id,
    ] {
        validate_api_identifier(identifier)?;
    }
    if let Some(identifier) = patch_id {
        validate_api_identifier(identifier)?;
    }
    let target = match &update {
        installer::UpdatePlan::Patch { target, .. }
        | installer::UpdatePlan::Full { target, .. } => target,
    };
    let manifest_url = install_manifest_url(&target.id, &target.artifact_id)
        .map_err(DistributionError::service_unavailable)?;
    let manifest = distribution_api_json(
        send(&client, client.get(manifest_url))
            .await
            .map_err(DistributionError::service_unavailable)?,
    )
    .await?;
    let patch_downloads = if let installer::UpdatePlan::Patch { patch, .. } = &update {
        let url = production_api_url(&format!("patches/{}/download", patch.id))
            .map_err(DistributionError::service_unavailable)?;
        Some(
            distribution_api_json(
                send(&client, client.post(url))
                    .await
                    .map_err(DistributionError::service_unavailable)?,
            )
            .await?,
        )
    } else {
        None
    };
    let fallback_artifact_id = match &update {
        installer::UpdatePlan::Patch {
            fallback_artifact_id,
            ..
        }
        | installer::UpdatePlan::Full {
            fallback_artifact_id,
            ..
        } => fallback_artifact_id,
    };
    let fallback_url = production_api_url(&format!("artifacts/{fallback_artifact_id}/download"))
        .map_err(DistributionError::service_unavailable)?;
    let fallback_download = distribution_api_json(
        send(&client, client.post(fallback_url))
            .await
            .map_err(DistributionError::service_unavailable)?,
    )
    .await?;
    Ok(installer::UpdateExecutionPlan {
        update,
        manifest,
        patch_downloads,
        fallback_download,
    })
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let api_state = ApiState::new().expect("failed to initialize the Manifold API client");
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(api_state)
        .manage(installer::InstallationManager::new())
        .manage(publisher::PublishUploadManager::new())
        .invoke_handler(tauri::generate_handler![
            application_info,
            list_store_games,
            current_user,
            request_otp,
            verify_otp,
            create_account,
            logout,
            list_library,
            resolve_latest_release,
            resolve_install_plan,
            resolve_update_plan,
            prepare_update_plan,
            publisher::list_publisher_studios,
            publisher::list_studio_games,
            publisher::list_game_releases,
            publisher::create_release_draft,
            publisher::update_release_draft,
            publisher::inspect_publish_archive,
            publisher::publish_release,
            publisher::cancel_publish_upload,
            installer::install_game,
            installer::updater::update_game,
            installer::cancel_installation,
            installer::list_installations,
            installer::launch_game,
            installer::list_running_games,
            installer::uninstall_game,
            installer::get_installation_preferences,
            installer::set_installation_preferences,
            installer::get_installation_diagnostics,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Manifold Desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn update_url_uses_the_frozen_wire_query_names() {
        let source = "11111111-1111-4111-8111-111111111111";
        let url = update_plan_url("capyvarias", source).unwrap();
        let query: HashMap<_, _> = url.query_pairs().into_owned().collect();
        assert_eq!(
            query.get("source_release_id").map(String::as_str),
            Some(source)
        );
        assert!(query.contains_key("platform"));
        assert!(query.contains_key("arch"));
        assert!(!query.contains_key("installed_release_id"));
    }

    #[test]
    fn accepts_known_environment() {
        assert_eq!(
            validated_application_info("production".into())
                .unwrap()
                .environment,
            "production"
        );
    }

    #[test]
    fn rejects_unknown_environment() {
        assert!(validated_application_info("preview".into()).is_err());
    }

    #[test]
    fn production_uses_the_shared_api_root() {
        assert_eq!(
            api_base_url("production", None).unwrap().as_str(),
            "https://manifoldpowered.com/api/v1/"
        );
    }

    #[test]
    fn builds_a_safe_store_catalog_url() {
        assert_eq!(
            store_games_url(Some("  cozy games  ")).unwrap().as_str(),
            "https://manifoldpowered.com/api/v1/games?limit=12&q=cozy+games"
        );
        assert!(store_games_url(Some(&"x".repeat(81))).is_err());
    }

    #[test]
    fn selects_the_resolved_artifact_when_requesting_a_manifest() {
        assert_eq!(
            install_manifest_url("release-1", "artifact-1")
                .unwrap()
                .as_str(),
            "https://manifoldpowered.com/api/v1/releases/release-1/manifest?schema_version=1&artifact_id=artifact-1"
        );
    }

    #[test]
    fn staging_requires_an_explicit_https_origin() {
        assert!(api_base_url("staging", None).is_err());
        assert!(api_base_url("staging", Some("http://staging.example.com")).is_err());
        assert_eq!(
            api_base_url("staging", Some("https://staging.example.com"))
                .unwrap()
                .as_str(),
            "https://staging.example.com/api/v1/"
        );
    }

    #[test]
    fn accepts_display_only_games_with_null_local_prices_and_external_offers() {
        let response: GamesApiResponse = serde_json::from_value(serde_json::json!({
            "games": [{
                "id": "game-1",
                "slug": "steam-game",
                "title": "Steam Game",
                "description": "Catalog contribution",
                "price": null,
                "developer_name": "Unclaimed",
                "status": "ONLY_DISPLAY",
                "ownership_status": "UNCLAIMED",
                "purchase_mode": "STEAM_ONLY",
                "external_offer": {
                    "provider": "STEAM",
                    "amount": "13.39",
                    "original_amount": "19.99",
                    "discount_percent": 33,
                    "currency": "USD",
                    "url": "https://store.steampowered.com/app/400/",
                    "captured_at": "2026-08-28T12:00:00.000Z"
                }
            }],
            "pagination": { "page": 1, "limit": 12, "total": 1, "pages": 1 },
            "currency": "BRL"
        }))
        .unwrap();

        let catalog = StoreCatalog::from(response);
        let game = &catalog.games[0];
        assert_eq!(game.price, None);
        assert_eq!(game.status, "ONLY_DISPLAY");
        assert_eq!(game.purchase_mode, "STEAM_ONLY");
        assert_eq!(
            game.external_offer.as_ref().unwrap().discount_percent,
            Some(33)
        );
    }
}
