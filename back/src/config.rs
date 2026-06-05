//! Configuration typée depuis l'environnement.

use std::sync::Arc;

#[derive(Debug, Clone)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub redis_url: String,
    pub clickhouse_url: String,
    pub clickhouse_db: String,
    pub clickhouse_user: String,
    pub clickhouse_password: String,
    pub jwt_secret: String,
    pub access_ttl_secs: i64,
    /// Origines autorisées pour CORS (allowlist). Vide ⇒ aucune origine cross-site (same-origin via nginx OK).
    pub cors_allowed_origins: Vec<String>,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Arc<Self>> {
        fn req(k: &str) -> anyhow::Result<String> {
            std::env::var(k)
                .map_err(|_| anyhow::anyhow!("variable d'environnement manquante : {k}"))
        }

        let jwt_secret = req("JWT_SECRET")?;
        validate_jwt_secret(&jwt_secret).map_err(|e| anyhow::anyhow!(e))?;

        Ok(Arc::new(Self {
            bind_addr: std::env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".into()),
            database_url: req("DATABASE_URL")?,
            redis_url: req("REDIS_URL")?,
            clickhouse_url: req("CLICKHOUSE_URL")?,
            clickhouse_db: std::env::var("CLICKHOUSE_DB").unwrap_or_else(|_| "quarity".into()),
            clickhouse_user: req("CLICKHOUSE_USER")?,
            clickhouse_password: req("CLICKHOUSE_PASSWORD")?,
            jwt_secret,
            access_ttl_secs: std::env::var("JWT_ACCESS_TTL_SECONDS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(900),
            cors_allowed_origins: std::env::var("CORS_ALLOWED_ORIGINS")
                .map(|s| {
                    s.split(',')
                        .map(|o| o.trim().to_string())
                        .filter(|o| !o.is_empty())
                        .collect()
                })
                .unwrap_or_else(|_| vec!["http://localhost:3000".to_string()]),
        }))
    }
}

/// Valide la robustesse du secret JWT au démarrage.
///
/// Rejette : trop court (< 32), placeholder évident (`change_me…`, valeur du `.env.example`),
/// ou entropie trop faible (caractères trop peu variés). Objectif : empêcher un déploiement
/// de partir avec un secret PUBLIC, qui rendrait tous les JWT forgeables.
fn validate_jwt_secret(secret: &str) -> Result<(), String> {
    if secret.len() < 32 {
        return Err("JWT_SECRET doit faire au moins 32 caractères".into());
    }

    // Marqueurs de placeholder (cf. `.env.example`). Comparaison insensible à la casse.
    let lower = secret.to_ascii_lowercase();
    const PLACEHOLDER_MARKERS: &[&str] = &[
        "change_me",
        "changeme",
        "placeholder",
        "your_secret",
        "to_change",
        "example",
        "secret_at_least_32",
    ];
    if let Some(marker) = PLACEHOLDER_MARKERS.iter().find(|m| lower.contains(**m)) {
        return Err(format!(
            "JWT_SECRET ressemble à un placeholder (contient « {marker} ») — \
             générez une vraie valeur aléatoire : openssl rand -hex 32"
        ));
    }

    // Garde-fou d'entropie : un secret trivial (« aaaa… », « ababab… ») a très peu de
    // caractères distincts ; une valeur aléatoire (ex. hex 64) en a une douzaine ou plus.
    let distinct = secret
        .chars()
        .collect::<std::collections::HashSet<char>>()
        .len();
    if distinct < 8 {
        return Err(format!(
            "JWT_SECRET trop peu varié ({distinct} caractères distincts) — \
             générez une vraie valeur aléatoire : openssl rand -hex 32"
        ));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::validate_jwt_secret;

    #[test]
    fn accepts_a_strong_random_secret() {
        // 40 caractères hex (≈ openssl rand) : longueur OK, pas de placeholder, entropie OK.
        assert!(validate_jwt_secret("7f3b9c1d4e8a2f6b5c0d9e3a1f7b4c8e2d6a0b9c").is_ok());
    }

    #[test]
    fn rejects_too_short() {
        assert!(validate_jwt_secret("trop_court").is_err());
    }

    #[test]
    fn rejects_exact_env_example_placeholder() {
        // Valeur livrée telle quelle dans `.env.example` : doit être refusée.
        assert!(validate_jwt_secret("change_me_jwt_secret_at_least_32_chars_long").is_err());
    }

    #[test]
    fn rejects_change_me_case_insensitive() {
        assert!(validate_jwt_secret("CHANGE_ME_0123456789_abcdef_0123456789").is_err());
    }

    #[test]
    fn rejects_low_entropy_secret() {
        // 40 caractères mais un seul distinct.
        assert!(validate_jwt_secret(&"a".repeat(40)).is_err());
    }
}
