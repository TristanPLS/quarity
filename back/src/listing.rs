//! Helpers de listing (B6) : pagination, tri par allowlist, échappement ILIKE.
//!
//! Conventions communes aux 4 listings CRUD (`users`, `organizations`,
//! `tracked_locations`, `alert_rules`) :
//! - `page` 1-indexée (défaut 1, max 1 000 000 — même borne anti-débordement
//!   que `/api/measurements`) ;
//! - `page_size` défaut 25, clampée 1..=100 (tables OLTP — `/api/measurements`
//!   garde ses bornes time-series propres : défaut 100, max 1000) ;
//! - `sort` : nom d'attribut API, préfixe `-` pour descendant (ex. `-created_at`),
//!   résolu STRICTEMENT par allowlist → aucune chaîne client n'atteint le SQL ;
//! - `q` : recherche sous-chaîne insensible à la casse (ILIKE), échappée.
//!
//! Réponse de page : `{ page, page_size, count, total, data }` — `count` = taille
//! de `data` (convention `/api/measurements`), `total` = lignes filtrées en base.

use serde::Deserialize;
use utoipa::IntoParams;
use validator::Validate;

use crate::error::AppError;

/// Paramètres de pagination/tri/recherche communs aux listings CRUD.
///
/// S'extrait À CÔTÉ des filtres propres à la ressource (deux extracteurs `Query`
/// sur la même query string — serde ignore les champs inconnus de part et d'autre).
#[derive(Debug, Deserialize, IntoParams, Validate)]
#[into_params(parameter_in = Query)]
pub struct ListParams {
    /// Page 1-indexée (défaut : 1 ; max 1 000 000).
    #[validate(range(min = 1, max = 1000000, message = "page attendue 1..=1000000"))]
    pub page: Option<u32>,
    /// Taille de page (défaut : 25 ; clampée 1..=100).
    #[validate(range(min = 1, max = 100, message = "page_size attendue 1..=100"))]
    pub page_size: Option<u32>,
    /// Tri : un attribut de l'allowlist de la ressource, préfixe `-` = descendant.
    #[param(example = "-created_at")]
    #[validate(length(max = 64, message = "sort : 64 caractères maximum"))]
    pub sort: Option<String>,
    /// Recherche sous-chaîne (insensible à la casse) sur les champs textuels de la ressource.
    #[validate(length(max = 200, message = "q : 200 caractères maximum"))]
    pub q: Option<String>,
}

/// Pagination résolue : valeurs effectives + LIMIT/OFFSET prêts à binder.
#[derive(Debug, Clone, Copy)]
pub struct Pagination {
    pub page: u32,
    pub page_size: u32,
    pub limit: i64,
    pub offset: i64,
}

impl ListParams {
    /// Défauts + clamps. L'offset est calculé en i64 : pas de débordement possible
    /// (`page` ≤ 10⁶ et `page_size` ≤ 100 par validation ⇒ offset < 10⁸).
    pub fn pagination(&self) -> Pagination {
        let page = self.page.unwrap_or(1).max(1);
        let page_size = self.page_size.unwrap_or(25).clamp(1, 100);
        Pagination {
            page,
            page_size,
            limit: i64::from(page_size),
            offset: (i64::from(page) - 1) * i64::from(page_size),
        }
    }

    /// Clause `ORDER BY` sûre : l'attribut API demandé est résolu via `allow`
    /// (paires `(nom_api, colonne_sql)`) — voir [`order_by`].
    pub fn order_by(&self, allow: &[(&str, &str)], default_api: &str) -> Result<String, AppError> {
        order_by(self.sort.as_deref(), allow, default_api)
    }

    /// Motif ILIKE prêt à binder (`%q%`, métacaractères échappés), si `q` est
    /// non vide après trim.
    pub fn like_pattern(&self) -> Option<String> {
        let q = self.q.as_deref()?.trim();
        if q.is_empty() {
            return None;
        }
        Some(format!("%{}%", escape_like(q)))
    }
}

/// Construit `ORDER BY <col> <ASC|DESC>, id ASC` à partir d'un tri API.
///
/// - `sort` : `"name"` (ascendant) ou `"-name"` (descendant) ; `None`/vide → `default_api`.
/// - `allow` : allowlist `(nom_api, colonne_sql)` — SEULE la colonne de l'allowlist
///   atteint le SQL, la chaîne client ne sert qu'à la sélectionner (anti-injection,
///   même principe que l'allowlist `parameter` de `/api/measurements`).
/// - `, id ASC` final : clé secondaire stable → pagination déterministe même quand
///   la colonne triée a des doublons.
///
/// Tri inconnu → 400 avec la liste des attributs acceptés.
pub fn order_by(
    sort: Option<&str>,
    allow: &[(&str, &str)],
    default_api: &str,
) -> Result<String, AppError> {
    let raw = match sort.map(str::trim) {
        None | Some("") => default_api,
        Some(s) => s,
    };
    let (key, dir) = match raw.strip_prefix('-') {
        Some(rest) => (rest, "DESC"),
        None => (raw, "ASC"),
    };
    let col = allow
        .iter()
        .find(|(api, _)| *api == key)
        .map(|(_, col)| *col)
        .ok_or_else(|| {
            let accepted: Vec<&str> = allow.iter().map(|(api, _)| *api).collect();
            AppError::BadRequest(format!(
                "sort invalide « {raw} » (attributs acceptés : {}, préfixe `-` pour descendant)",
                accepted.join(", ")
            ))
        })?;
    Ok(format!("ORDER BY {col} {dir}, id ASC"))
}

/// Échappe les métacaractères LIKE/ILIKE (`\`, `%`, `_`) d'une saisie utilisateur.
/// La valeur reste TOUJOURS bindée (`$n`) — l'échappement neutralise seulement les
/// jokers, il ne « sécurise » pas une interpolation (qui reste interdite).
pub fn escape_like(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::{escape_like, order_by, ListParams};

    const ALLOW: &[(&str, &str)] = &[("name", "name"), ("created_at", "created_at")];

    fn params(page: Option<u32>, page_size: Option<u32>) -> ListParams {
        ListParams {
            page,
            page_size,
            sort: None,
            q: None,
        }
    }

    #[test]
    fn pagination_defaults_and_clamps() {
        let p = params(None, None).pagination();
        assert_eq!((p.page, p.page_size, p.limit, p.offset), (1, 25, 25, 0));

        let p = params(Some(3), Some(40)).pagination();
        assert_eq!((p.limit, p.offset), (40, 80));

        // Hors bornes (la validation rejette en amont — le clamp reste un filet).
        let p = params(Some(1), Some(10_000)).pagination();
        assert_eq!(p.page_size, 100);
    }

    #[test]
    fn pagination_offset_never_overflows() {
        // Pire cas permis par la validation : page 10⁶ × page_size 100.
        let p = params(Some(1_000_000), Some(100)).pagination();
        assert_eq!(p.offset, 99_999_900);
    }

    #[test]
    fn order_by_resolves_allowlist_and_direction() {
        assert_eq!(
            order_by(Some("name"), ALLOW, "name").unwrap(),
            "ORDER BY name ASC, id ASC"
        );
        assert_eq!(
            order_by(Some("-created_at"), ALLOW, "name").unwrap(),
            "ORDER BY created_at DESC, id ASC"
        );
        // None / vide → défaut.
        assert_eq!(
            order_by(None, ALLOW, "-created_at").unwrap(),
            "ORDER BY created_at DESC, id ASC"
        );
        assert_eq!(
            order_by(Some("  "), ALLOW, "name").unwrap(),
            "ORDER BY name ASC, id ASC"
        );
    }

    #[test]
    fn order_by_rejects_anything_outside_allowlist() {
        for evil in ["id; DROP TABLE users", "name,created_at", "unknown", "--"] {
            assert!(
                order_by(Some(evil), ALLOW, "name").is_err(),
                "doit rejeter {evil:?}"
            );
        }
    }

    #[test]
    fn escape_like_neutralizes_wildcards() {
        assert_eq!(escape_like("50%_a\\b"), "50\\%\\_a\\\\b");
        assert_eq!(escape_like("riviera"), "riviera");
    }

    #[test]
    fn like_pattern_trims_and_skips_blank() {
        let mut p = params(None, None);
        p.q = Some("  école 10%  ".into());
        assert_eq!(p.like_pattern().as_deref(), Some("%école 10\\%%"));
        p.q = Some("   ".into());
        assert_eq!(p.like_pattern(), None);
    }
}
