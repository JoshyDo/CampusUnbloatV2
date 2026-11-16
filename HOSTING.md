# CampusUnbloatV2 selbst hosten – kurze Anleitung

Diese Anleitung richtet sich an jemanden, der CampusUnbloatV2 auf einem
eigenen Server (z. B. VPS, Homeserver) für mehrere Nutzer bereitstellen will.
Du brauchst dafür **keine Zugangsdaten von uns** – jede Person loggt sich mit
ihrem eigenen Campus-Dual-Account direkt in der App ein.

## Voraussetzungen

- Ein Server mit Linux (empfohlen) oder Windows, auf dem du Software
  installieren kannst.
- Eine Domain, die auf den Server zeigt (z. B. `campus.deine-domain.de`),
  falls du es öffentlich erreichbar machen willst.

## 1. Code holen

```bash
git clone https://github.com/JoshyDo/CampusUnbloatV2.git
cd CampusUnbloatV2
```

## 2. Rust-Toolchain installieren

Falls noch nicht vorhanden: [rustup.rs](https://rustup.rs) folgen, dann:

```bash
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

## 3. Bauen

```bash
cd web
trunk build --release
cd ..
cargo build --release -p campusunbloat-server
```

Das Frontend (WASM) und der Server werden damit gebaut. Nach jeder
zukünftigen Code-Änderung müssen beide Schritte wiederholt werden (der
Server berechnet beim Start eine Sicherheitsrichtlinie passend zum aktuell
gebauten Frontend – ein Neustart danach ist also Pflicht).

## 4. Konfiguration (Umgebungsvariablen)

Nur diese Werte sind relevant, alle anderen haben sinnvolle Standardwerte:

| Variable | Pflicht? | Zweck |
|---|---|---|
| `CAMPUSUNBLOAT_PORT` | nein (Standard `3000`) | Port, auf dem der Server lokal lauscht |
| `CAMPUSUNBLOAT_ALLOWED_HOST` | **ja, bei echter Domain** | z. B. `campus.deine-domain.de` – ohne das blockt der Server alle Anfragen, die nicht über `localhost`/`127.0.0.1` kommen |
| `CAMPUSUNBLOAT_SECURE_COOKIES` | **ja, sobald HTTPS aktiv ist** | auf `1` setzen, sobald ein Reverse-Proxy TLS terminiert |
| `CAMPUSUNBLOAT_SECRET` | nein | 64 Hex-Zeichen; nur nötig, wenn mehrere Server-Instanzen den gleichen Schlüssel für "Angemeldet bleiben" teilen sollen. Ohne das wird automatisch eine lokale `remember.key`-Datei erzeugt – reicht für den Normalfall. |
| `CAMPUSUNBLOAT_DATA_DIR` | nein | Ordner für die automatisch erzeugte `remember.key` (Standard: aktuelles Verzeichnis) |

Beispiel für den Start (Linux, bash):

```bash
export CAMPUSUNBLOAT_PORT=3000
export CAMPUSUNBLOAT_ALLOWED_HOST="campus.deine-domain.de"
export CAMPUSUNBLOAT_SECURE_COOKIES=1
./target/release/campusunbloat-server
```

## 5. Reverse-Proxy mit HTTPS davorschalten (wichtig!)

Der Server selbst spricht nur unverschlüsseltes HTTP auf `127.0.0.1`. Für
den öffentlichen Betrieb **muss** ein Reverse-Proxy mit gültigem
TLS-Zertifikat davor, sonst gehen Campus-Dual-Zugangsdaten unverschlüsselt
übers Netz. Am einfachsten mit [Caddy](https://caddyserver.com) (holt sich
automatisch ein Let's-Encrypt-Zertifikat):

```
# /etc/caddy/Caddyfile
campus.deine-domain.de {
    reverse_proxy 127.0.0.1:3000
}
```

Dann `caddy run` (oder als systemd-Service einrichten). Fertig – Caddy
kümmert sich um HTTPS, der Rust-Server bleibt intern auf Port 3000.

## 6. Dauerhaft laufen lassen

Damit der Server Neustarts/Abstürze übersteht, z. B. als systemd-Service
(Linux):

```ini
# /etc/systemd/system/campusunbloat.service
[Unit]
Description=CampusUnbloatV2
After=network.target

[Service]
WorkingDirectory=/pfad/zu/CampusUnbloatV2
Environment=CAMPUSUNBLOAT_PORT=3000
Environment=CAMPUSUNBLOAT_ALLOWED_HOST=campus.deine-domain.de
Environment=CAMPUSUNBLOAT_SECURE_COOKIES=1
ExecStart=/pfad/zu/CampusUnbloatV2/target/release/campusunbloat-server
Restart=on-failure
User=campusunbloat

[Install]
WantedBy=multi-user.target
```

```bash
sudo systemctl enable --now campusunbloat
```

## Was wir dir NICHT geben müssen

- Kein Campus-Dual-Passwort, kein Session-Token – jede Person loggt sich
  selbst über die App ein.
- Keine Secrets/Schlüssel – `remember.key` erzeugt der Server beim ersten
  Start automatisch und selbstständig auf deinem Server.

## Updates einspielen

```bash
git pull
cd web && trunk build --release && cd ..
cargo build --release -p campusunbloat-server
sudo systemctl restart campusunbloat   # falls als Service eingerichtet
```

Mehr Details zur Sicherheitsarchitektur (Cookies, CSP, Rate-Limiting) stehen
im Abschnitt "Sicherheit" der [README.md](README.md).
