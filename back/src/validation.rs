//! Validation des inputs au boundary (A5) : extracteurs `ValidatedJson` / `ValidatedQuery`.
//!
//! Contrat d'erreurs à la frontière (cohérent avec la doc OpenAPI posée en A2) :
//! - Rejets de l'**extracteur axum** (JSON malformé → 400, `Content-Type` non JSON → 415,
//!   champs manquants/mal typés → 422, query mal typée → 400) : **inchangés**, transmis
//!   tels quels (corps texte).
//! - Échec de **validation déclarative** (`validator`, après désérialisation) : 400
//!   `ErrorBody` (code `bad_request`), message agrégé par champ — même convention que
//!   les 400 applicatifs existants (ex. allowlist `parameter`).

use axum::extract::{FromRequest, FromRequestParts, Query, Request};
use axum::http::request::Parts;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde::de::DeserializeOwned;
use validator::Validate;

use crate::error::AppError;

/// `Json<T>` + `T::validate()`. À placer en DERNIER argument du handler (consomme le corps).
pub struct ValidatedJson<T>(pub T);

impl<S, T> FromRequest<S> for ValidatedJson<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
{
    type Rejection = Response;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let Json(value) = Json::<T>::from_request(req, state)
            .await
            .map_err(IntoResponse::into_response)?;
        value
            .validate()
            .map_err(|e| AppError::BadRequest(format!("validation : {e}")).into_response())?;
        Ok(ValidatedJson(value))
    }
}

/// `Query<T>` + `T::validate()`.
pub struct ValidatedQuery<T>(pub T);

impl<S, T> FromRequestParts<S> for ValidatedQuery<T>
where
    S: Send + Sync,
    T: DeserializeOwned + Validate,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(value) = Query::<T>::from_request_parts(parts, state)
            .await
            .map_err(IntoResponse::into_response)?;
        value
            .validate()
            .map_err(|e| AppError::BadRequest(format!("validation : {e}")).into_response())?;
        Ok(ValidatedQuery(value))
    }
}

/// Parse une datetime « raisonnable » au boundary : `YYYY-MM-DD` (format du front,
/// `<input type="date">`), `YYYY-MM-DD[T ]HH:MM[:SS[.fff]]`, ou RFC 3339 (`...Z`/offset).
/// Les espaces de bord sont tolérés (trim explicite — chrono tolère déjà des espaces
/// de tête de façon implicite, autant que ce soit documenté et symétrique).
/// Plus STRICT que `parseDateTime64BestEffort` de ClickHouse : c'est voulu — avant A5,
/// une chaîne arbitraire traversait jusqu'à ClickHouse et ressortait en 500.
pub fn parse_datetime_ish(s: &str) -> Option<chrono::NaiveDateTime> {
    use chrono::{DateTime, NaiveDate, NaiveDateTime};

    let s = s.trim();
    if let Ok(dt) = DateTime::parse_from_rfc3339(s) {
        return Some(dt.naive_utc());
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S%.f",
        "%Y-%m-%dT%H:%M:%S%.f",
        "%Y-%m-%d %H:%M",
        "%Y-%m-%dT%H:%M",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(s, fmt) {
            return Some(dt);
        }
    }
    if let Ok(d) = NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return d.and_hms_opt(0, 0, 0);
    }
    None
}

/// Validateur `validator` pour un champ datetime (cf. [`parse_datetime_ish`]).
pub fn validate_datetime_ish(s: &str) -> Result<(), validator::ValidationError> {
    parse_datetime_ish(s).map(|_| ()).ok_or_else(|| {
        validator::ValidationError::new("datetime_invalide").with_message(
            "format attendu : YYYY-MM-DD, YYYY-MM-DD[T ]HH:MM[:SS[.fff]] ou RFC 3339".into(),
        )
    })
}

/// Rejette les chaînes vides ou composées uniquement d'espaces (une `length(min = 1)`
/// seule laisse passer `"   "` — la règle doit rester déclarative, pas dans le handler).
pub fn validate_not_blank(s: &str) -> Result<(), validator::ValidationError> {
    if s.trim().is_empty() {
        return Err(validator::ValidationError::new("blank")
            .with_message("ne doit pas être vide ni composé uniquement d'espaces".into()));
    }
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Allowlists déclaratives des DTOs CRUD (B6). Chacune est le MIROIR d'un CHECK
// du schéma (0001_init.sql) : la base reste l'autorité, le boundary évite juste
// qu'une valeur invalide parte en transaction pour revenir en erreur SQL.
// ─────────────────────────────────────────────────────────────────────────────

fn validate_in(
    value: &str,
    allowed: &'static [&'static str],
    code: &'static str,
) -> Result<(), validator::ValidationError> {
    if allowed.contains(&value) {
        return Ok(());
    }
    Err(validator::ValidationError::new(code)
        .with_message(format!("valeurs acceptées : {}", allowed.join(", ")).into()))
}

/// Codes polluants (miroir du CHECK `parameters.code`).
pub fn validate_parameter_code(s: &str) -> Result<(), validator::ValidationError> {
    validate_in(s, &["pm25", "pm10", "no2", "o3", "so2", "co"], "parameter")
}

/// Comparateurs de règle d'alerte (miroir du CHECK `alert_rules.comparator`).
pub fn validate_comparator(s: &str) -> Result<(), validator::ValidationError> {
    validate_in(s, &[">", ">="], "comparator")
}

/// Sévérités de règle d'alerte (miroir du CHECK `alert_rules.severity`).
pub fn validate_severity(s: &str) -> Result<(), validator::ValidationError> {
    validate_in(s, &["info", "warning", "critical"], "severity")
}

/// Codes de rôle RBAC (miroir du CHECK `roles.code`).
pub fn validate_role_code(s: &str) -> Result<(), validator::ValidationError> {
    validate_in(s, &["admin", "gestionnaire", "lecteur"], "role")
}

/// Segments d'organisation (miroir du CHECK `organizations.segment`).
pub fn validate_segment(s: &str) -> Result<(), validator::ValidationError> {
    validate_in(s, &["B2G", "B2B", "B2B2C"], "segment")
}

/// Slug d'organisation (miroir du CHECK `organizations.slug` : `^[a-z0-9-]{2,64}$`),
/// vérifié sans crate regex.
pub fn validate_org_slug(s: &str) -> Result<(), validator::ValidationError> {
    let len_ok = (2..=64).contains(&s.len());
    let chars_ok = s
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-');
    if len_ok && chars_ok {
        return Ok(());
    }
    Err(validator::ValidationError::new("slug")
        .with_message("2 à 64 caractères parmi [a-z0-9-]".into()))
}

#[cfg(test)]
mod tests {
    use super::parse_datetime_ish;

    #[test]
    fn accepts_front_and_api_formats() {
        // Format du front (<input type="date">) et variantes clients API.
        for ok in [
            "2026-04-01",
            "2026-04-01 12:30",
            "2026-04-01T12:30",
            "2026-04-01 12:30:45",
            "2026-04-01T12:30:45.123",
            "2026-04-01T12:30:45Z",
            "2026-04-01T12:30:45+02:00",
        ] {
            assert!(parse_datetime_ish(ok).is_some(), "doit accepter {ok}");
        }
    }

    #[test]
    fn rejects_garbage() {
        // Avant A5, ces chaînes traversaient jusqu'à ClickHouse → 500.
        for ko in ["", "n'importe quoi", "2026-13-45", "01/04/2026", "now()"] {
            assert!(parse_datetime_ish(ko).is_none(), "doit rejeter {ko}");
        }
    }

    #[test]
    fn trims_edge_whitespace_but_rejects_unicode_lookalikes() {
        // Tolérance documentée : espaces de bord (trim explicite).
        for ok in [
            " 2026-04-01",
            "2026-04-01 ",
            "\t2026-04-01",
            " 2026-04-01T12:30:45Z ",
        ] {
            assert!(parse_datetime_ish(ok).is_some(), "doit accepter {ok:?}");
        }
        // Pas de bypass par sosies Unicode (chiffres pleine chasse, tiret cadratin,
        // espace insécable interne).
        for ko in ["２０２６-04-01", "2026—04—01", "2026-04\u{a0}01"] {
            assert!(parse_datetime_ish(ko).is_none(), "doit rejeter {ko:?}");
        }
    }

    #[test]
    fn date_only_is_midnight() {
        let dt = parse_datetime_ish("2026-04-01").expect("date simple");
        assert_eq!(dt.format("%H:%M:%S").to_string(), "00:00:00");
    }

    #[test]
    fn allowlists_mirror_schema_checks() {
        use super::*;
        assert!(validate_parameter_code("pm25").is_ok());
        assert!(validate_parameter_code("PM25").is_err()); // sensible à la casse, comme le CHECK
        assert!(validate_comparator(">=").is_ok());
        assert!(validate_comparator("<").is_err()); // seuls les dépassements existent (US-02)
        assert!(validate_severity("critical").is_ok());
        assert!(validate_severity("fatal").is_err());
        assert!(validate_role_code("lecteur").is_ok());
        assert!(validate_role_code("root").is_err());
        assert!(validate_segment("B2G").is_ok());
        assert!(validate_segment("b2g").is_err());
    }

    #[test]
    fn org_slug_rules() {
        use super::validate_org_slug;
        assert!(validate_org_slug("agglo-riviera").is_ok());
        assert!(validate_org_slug("e2").is_ok()); // borne basse : 2 caractères
        assert!(validate_org_slug("a").is_err()); // trop court
        assert!(validate_org_slug("Agglo").is_err()); // majuscule
        assert!(validate_org_slug("a_b").is_err()); // underscore hors charte
        assert!(validate_org_slug(&"x".repeat(65)).is_err()); // trop long
    }
}
