use serde::Serialize;

const ENVIRONMENTS: [&str; 3] = ["development", "staging", "production"];
const API_PATH: &str = "/api/v1";
const DEVELOPMENT_API_ORIGIN: &str = "http://localhost:3000";
const PRODUCTION_API_ORIGIN: &str = "https://manifoldpowered.com";

#[derive(Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
struct ApplicationInfo {
    version: &'static str,
    environment: String,
    platform: &'static str,
    architecture: &'static str,
}

fn validated_application_info(environment: String) -> Result<ApplicationInfo, String> {
    if !ENVIRONMENTS.contains(&environment.as_str()) {
        return Err(format!(
            "unsupported application environment: {environment}"
        ));
    }
    let staging_origin = std::env::var("MANIFOLD_API_BASE_URL").ok();
    let _api_base_url = api_base_url(&environment, staging_origin.as_deref())?;
    let (platform, architecture) = desktop_target()?;
    Ok(ApplicationInfo {
        version: env!("CARGO_PKG_VERSION"),
        environment,
        platform,
        architecture,
    })
}

fn desktop_target() -> Result<(&'static str, &'static str), String> {
    let platform = match std::env::consts::OS {
        "windows" => "WINDOWS",
        "macos" => "MAC",
        "linux" => "LINUX",
        other => return Err(format!("unsupported desktop platform: {other}")),
    };
    let architecture = match std::env::consts::ARCH {
        "x86_64" => "X86_64",
        "aarch64" => "AARCH64",
        other => return Err(format!("unsupported desktop architecture: {other}")),
    };
    Ok((platform, architecture))
}

/// Resolves the contract's API root inside the trusted process. Staging has no
/// canonical public origin in the upstream contract and must be configured
/// explicitly by the launch environment.
fn api_base_url(environment: &str, staging_origin: Option<&str>) -> Result<url::Url, String> {
    let origin = match environment {
        "development" => DEVELOPMENT_API_ORIGIN,
        "production" => PRODUCTION_API_ORIGIN,
        "staging" => staging_origin.ok_or("MANIFOLD_API_BASE_URL is required for staging")?,
        other => return Err(format!("unsupported application environment: {other}")),
    };
    let origin = url::Url::parse(origin).map_err(|error| format!("invalid API origin: {error}"))?;
    if origin.path() != "/" || origin.query().is_some() || origin.fragment().is_some() {
        return Err("API origin must not contain a path, query, or fragment".into());
    }
    if environment != "development" && origin.scheme() != "https" {
        return Err(format!("{environment} API origin must use HTTPS"));
    }
    origin
        .join(&format!("{API_PATH}/"))
        .map_err(|error| format!("invalid API URL: {error}"))
}

#[tauri::command]
fn application_info(environment: String) -> Result<ApplicationInfo, String> {
    validated_application_info(environment)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // Privileged operations must be added as narrowly scoped commands here;
        // the frontend is intentionally not granted shell or filesystem plugins.
        .invoke_handler(tauri::generate_handler![application_info])
        .run(tauri::generate_context!())
        .expect("error while running Manifold Desktop");
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn uses_the_versioned_api_root() {
        assert_eq!(
            api_base_url("production", None).unwrap().as_str(),
            "https://manifoldpowered.com/api/v1/"
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
}
