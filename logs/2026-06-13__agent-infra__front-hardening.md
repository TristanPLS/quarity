# Log — agent-infra — 2026-06-13 — front-hardening

## 12:05 CEST — Durcissement front Docker : non-root + healthcheck + gzip + timeout WS

- **Agent / rôle** : agent-infra
- **Jalon / tâche** : Jalon 4 (durcissement) — constats A2 / P1-2 / P2 des audits 06-10 & 06-12.
- **Contexte** : le front était le seul conteneur en **root** (asymétrie avec le back déjà durci) et le seul **sans healthcheck** ni `depends_on: service_healthy` → au boot, si le back est plus lent, le navigateur tape `/api/me` dans le vide = écran d'erreur transitoire visible en démo (A2). De plus, nginx ne compressait pas les assets (P2) et n'avait pas de timeout proxy explicite pour la WebSocket d'alertes.
- **Actions** :
  - `front/Dockerfile` : passage à l'image officielle `nginxinc/nginx-unprivileged:1.27-alpine` (master en uid 101 `nginx`, écoute 8080 non privilégié) → plus aucun process en root côté front. `EXPOSE 8080`.
  - `front/nginx.conf` : `listen 8080` ; bloc `gzip on` (P2 : JS/CSS/JSON/svg/wasm, `gzip_min_length 1000`, `gzip_vary on`) ; `proxy_read_timeout`/`proxy_send_timeout 3600s` sur `location /api/` (garde la WS ouverte entre deux pings back de 30 s — sans ça, le défaut nginx de 60 s couperait les connexions idle).
  - `docker-compose.yml` (service `front`) : port hôte `3000:8080`, `depends_on: {back: {condition: service_healthy}}`, `healthcheck` `wget -q --spider http://127.0.0.1:8080/` (127.0.0.1 et **pas** localhost : busybox wget tenterait ::1 d'abord, or `listen 8080` n'écoute qu'en IPv4).
- **Fichiers touchés** :
  - `front/Dockerfile` (modifié)
  - `front/nginx.conf` (modifié)
  - `docker-compose.yml` (modifié)
- **Résultat** : OK — smoke local complet réussi.
- **Vérifs** : `docker compose config -q` OK · `docker compose build front` OK (image non-root) · recréé + **smoke** : `curl localhost:3000/` → **200** ; `docker exec quarity_front id` → **uid=101(nginx)** (non-root) ; `Content-Encoding: gzip` sur `/assets/*.js` ; `nginx -t` → syntax ok ; `docker inspect` santé → **healthy** (probes exit 0) ; `depends_on` a bien attendu `back` healthy avant de démarrer le front.
- **Prochaine étape** : #3 CI Vitest + ESLint (A1/A6), puis #4 borne/parallélisation dose (A3/P3), #5 intégrité AQI (A4), #6 trigger T8 (A5).
- **Action Git suggérée à l'humain** :
  > ─────────────────────────────────────────────
  > Prochaine étape avant de continuer (à exécuter par toi) :
  > Script : quarity/commit-fix-front-hardening.cmd
  > Branche cible : fix/infra-front-hardening (depuis origin/dev)
  > fetch + switch -c, git add des 3 fichiers + ce log, commit ASCII, push, gh pr create vers dev.
  > Puis : attendre la CI verte, squash merge.
  > ─────────────────────────────────────────────
