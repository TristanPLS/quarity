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

/// Lit le refresh SANS le consommer ; renvoie (user_id, org_id) si présent ET lisible.
/// Réservé au logout, qui doit vérifier la PROPRIÉTÉ avant de décider de supprimer (le
/// supprimer d'office ferait du logout un oracle de validité des tokens d'autrui).
/// La rotation (refresh) utilise `take_refresh` (consommation atomique).
/// Un payload corrompu est tracé (warn) puis traité comme un token invalide —
/// jamais de repli silencieux vers (0, 0).
pub async fn read_refresh(
    conn: &mut ConnectionManager,
    token: &str,
) -> redis::RedisResult<Option<(i64, i64)>> {
    let key = format!("refresh:{token}");
    let raw: Option<String> = conn.get(&key).await?;
    match raw {
        Some(raw) => parse_refresh_payload(&raw),
        None => Ok(None),
    }
}

/// Consomme ATOMIQUEMENT un refresh token (GETDEL) : lit (user_id, org_id) ET supprime la
/// clé en UNE seule commande Redis. Garantit qu'un token n'est consommé qu'une seule fois,
/// même sous deux requêtes concurrentes (retry réseau, double-onglet, rejeu d'un token
/// volé) : Redis étant mono-thread, exactement un appelant reçoit la valeur, l'autre reçoit
/// `None`. C'est la primitive de rotation inviolable utilisée par `routes::auth::refresh`
/// (un read puis un delete séparés laisseraient une fenêtre de course entre les deux).
pub async fn take_refresh(
    conn: &mut ConnectionManager,
    token: &str,
) -> redis::RedisResult<Option<(i64, i64)>> {
    let key = format!("refresh:{token}");
    let raw: Option<String> = conn.get_del(&key).await?;
    match raw {
        Some(raw) => parse_refresh_payload(&raw),
        None => Ok(None),
    }
}

/// Parse le payload `user_id:org_id` d'un refresh. Un payload illisible est tracé (warn —
/// sans le token, qui est un secret) puis traité comme invalide ; jamais de repli (0, 0).
fn parse_refresh_payload(raw: &str) -> redis::RedisResult<Option<(i64, i64)>> {
    let mut it = raw.splitn(2, ':');
    let uid = it.next().and_then(|s| s.parse::<i64>().ok());
    let oid = it.next().and_then(|s| s.parse::<i64>().ok());
    match (uid, oid) {
        (Some(uid), Some(oid)) => Ok(Some((uid, oid))),
        _ => {
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
