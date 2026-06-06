//! Refresh tokens + compteurs de rate-limit en Redis.
//! Clé `refresh:{token}` → `user_id:org_id`, TTL configurable (`REFRESH_TTL_SECS`, déf. 7 j).
//! NB : depuis le durcissement S4, le refresh token est INDÉPENDANT du `jti` du JWT d'accès.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;

pub async fn store_refresh(
    conn: &mut ConnectionManager,
    token: &str,
    user_id: i64,
    org_id: i64,
    ttl_secs: u64,
) -> redis::RedisResult<()> {
    let key = format!("refresh:{token}");
    let val = format!("{user_id}:{org_id}");
    conn.set_ex::<_, _, ()>(key, val, ttl_secs).await?;
    Ok(())
}

/// Lit le refresh ; renvoie (user_id, org_id) si présent ET lisible.
/// Un payload corrompu est tracé (warn) puis traité comme un token invalide —
/// jamais de repli silencieux vers (0, 0).
pub async fn read_refresh(
    conn: &mut ConnectionManager,
    token: &str,
) -> redis::RedisResult<Option<(i64, i64)>> {
    let key = format!("refresh:{token}");
    let raw: Option<String> = conn.get(&key).await?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let mut it = raw.splitn(2, ':');
    let uid = it.next().and_then(|s| s.parse::<i64>().ok());
    let oid = it.next().and_then(|s| s.parse::<i64>().ok());
    match (uid, oid) {
        (Some(uid), Some(oid)) => Ok(Some((uid, oid))),
        _ => {
            // On ne logge PAS le token (secret) — seulement le payload corrompu.
            tracing::warn!(payload = %raw, "payload refresh illisible en Redis — token rejeté");
            Ok(None)
        }
    }
}

pub async fn delete_refresh(conn: &mut ConnectionManager, token: &str) -> redis::RedisResult<()> {
    let key = format!("refresh:{token}");
    conn.del::<_, ()>(key).await?;
    Ok(())
}

/// Rate-limit « fenêtre fixe » : INCR + EXPIRE à la première incrémentation.
/// Renvoie le compteur courant de la fenêtre ; l'appelant compare à sa limite.
/// Simple et suffisant ici (légère tolérance au bord de fenêtre, pas de dépendance en plus).
pub async fn rate_limit_hit(
    conn: &mut ConnectionManager,
    key: &str,
    window_secs: i64,
) -> redis::RedisResult<u64> {
    let count: u64 = conn.incr(key, 1i64).await?;
    if count == 1 {
        // Première frappe de la fenêtre → on arme l'expiration du compteur.
        conn.expire::<_, ()>(key, window_secs).await?;
    }
    Ok(count)
}
