# ---- build ----
FROM node:22-alpine AS builder
WORKDIR /app
COPY package.json package-lock.json* ./
RUN npm ci || npm install
COPY . .
RUN npm run build

# ---- runtime (nginx NON-ROOT sert le statique + proxy /api vers le back) ----
# Image officielle nginx-unprivileged : le master tourne en uid 101 (`nginx`) et
# ecoute sur 8080 (port non privilegie). Durcissement A2 / P1-2 : plus aucun
# process en root cote front (symetrie avec le back, deja non-root).
# Le COPY ecrase le default.conf de l'image (pas besoin de rm prealable).
FROM nginxinc/nginx-unprivileged:1.27-alpine AS runtime
COPY nginx.conf /etc/nginx/conf.d/default.conf
COPY --from=builder /app/dist /usr/share/nginx/html
EXPOSE 8080
