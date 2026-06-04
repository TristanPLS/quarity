//! Refresh tokens en Redis. Clé `refresh:{jti}` → `user_id:org_id`, TTL 7 j.

use redis::aio::ConnectionManager;
use redis::AsyncCommands;

const REFRESH_TTL_SECS: u64 = 7 * 24 * 60 * 60;

pub async fn store_refresh(
    conn: &mut ConnectionManager,
    jti: &str,
    user_id: i64,
    org_id: i64,
) -> redis::RedisResult<()> {
    let key = format!("refresh:{jti}");
    let val = format!("{user_id}:{org_id}");
    conn.set_ex::<_, _, ()>(key, val, REFRESH_TTL_SECS).await?;
    Ok(())
}

/// Lit le refresh ; renvoie (user_id, org_id) si présent/valide.
pub async fn read_refresh(
    conn: &mut ConnectionManager,
    jti: &str,
) -> redis::RedisResult<Option<(i64, i64)>> {
    let key = format!("refresh:{jti}");
    let raw: Option<String> = conn.get(&key).await?;
    let Some(raw) = raw else {
        return Ok(None);
    };
    let mut it = raw.splitn(2, ':');
    let uid: i64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let oid: i64 = it.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    Ok(Some((uid, oid)))
}

pub async fn delete_refresh(conn: &mut ConnectionManager, jti: &str) -> redis::RedisResult<()> {
    let key = format!("refresh:{jti}");
    conn.del::<_, ()>(key).await?;
    Ok(())
}
