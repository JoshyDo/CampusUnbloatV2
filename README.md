# CampusUnbloatV2

Eine lokale Web-App für Campus Dual V2. Das Backend und die Browser-Oberfläche
sind in Rust geschrieben; die Oberfläche wird mit WebAssembly ausgeführt.

## Voraussetzungen

- Rust mit Cargo und einem funktionierenden C/C++-Compiler
  (unter Windows: Visual Studio Build Tools mit MSVC).
- Internetzugang für Campus Dual und die Mensa-Daten.
- Das Rust-Ziel `wasm32-unknown-unknown` und das Build-Werkzeug Trunk.

## Start unter Windows

In PowerShell einmalig das WebAssembly-Ziel und Trunk installieren:

```powershell
rustup target add wasm32-unknown-unknown
cargo install --locked trunk
```

Danach die Oberfläche bauen und den Server starten:

```powershell
cd C:\Pfad\zu\CampusUnbloatV2
cd web
trunk build --release
cd ..
cargo run --release -p campusunbloat-server
```

Anschließend `http://localhost:3000` im Browser öffnen. Nach Änderungen an
der Oberfläche `trunk build --release` erneut ausführen und den Server
neu starten: Er ermittelt beim Start die erlaubten Hashes des generierten
WebAssembly-Startcodes für die Browser-Sicherheitsrichtlinie. Für lokale
Entwicklung genügt auch `trunk build` und `cargo run -p campusunbloat-server`.

Falls Port 3000 bereits belegt ist, vor dem Start einen anderen Port setzen:

```powershell
$env:CAMPUSUNBLOAT_PORT = "3001"
cargo run -p campusunbloat-server
```

Dann `http://localhost:3001` öffnen. Der Server lauscht ausschließlich auf
`127.0.0.1`; ein öffentlicher Betrieb oder Zugriff aus dem lokalen Netzwerk
ist nicht vorgesehen. Die früheren Dienste auf Port 8080 und 9090 sowie Node.js
werden für diese Anwendung nicht benötigt.

## Funktionen und Grenzen

- Anmeldung bei Campus Dual V2, persönliche Daten, Fachsemester und ECTS.
- Noten, gruppiert nach Zeitraum, mit ECTS-gewichtetem Durchschnitt.
- Wochenkalender mit Modul, Raum und Dozent; Prüfungen sind rot markiert.
- Benachrichtigungen zu anstehenden Prüfungen, neuen Ergebnissen und neuen
  Prüfungsangeboten (gelesene Angebote merkt sich der Browser).
- Speiseplan über OpenMensa (Standard: Schönauer Straße) mit Tagespfeilen.
- Prüfungsanmeldung und -abmeldung (`ZBA_EXAM_REGISTRATION_SRV`) mit
  zweistufiger Bestätigung. Beides wird wirklich an Campus Dual gesendet.
  Nach jeder Aktion kann ein Beleg als PNG gespeichert werden; er ist kein
  offizieller Nachweis. Die Antwortformate stammen nur aus den
  Metadaten und sind mit einem echten Konto noch ungetestet; die
  Anfrage-Logik ist gegen einen lokalen Fake-Server getestet.
- Für den Blockplan (Theorie- und Praxisphasen) wurde keine V2-Datenquelle
  gefunden. Das Launchpad bietet nur Persönliche Daten, Studentendokumente,
  Studienfortschritt, Anmeldung Wahlpflichtmodule, Prüfungen, Ergebnisse
  und Stundenplan. Phasen werden deshalb nicht geschätzt.

Die Nutzung der Campus-Dual-Schnittstellen wurde aus dem bisherigen Adapter
übernommen. Die neu zusammengestellte App muss mit einem echten Konto noch
gegen den aktuellen Stand des Portals getestet werden.

### Studentendokumente und Wahlpflichtmodule

- Studentendokumente: Die Dokumente (z. B. Studienbescheinigung) lassen sich pro Semester als PDF laden (`ZBA_CM_STUDENT_DOCUMENTS_SRV`).
- Wahlpflichtmodule: Ein eigener Block (eingeklappt, solange keine Anmeldung offen ist) und eine Benachrichtigung, sobald Module anmeldbar sind (`ZBA_OPT_MODULES_SRV`). Der Block ist nur lesend; die Anmeldung erfolgt in Campus Dual, weil der Buchungsaufruf nicht bekannt ist. Die Feldzuordnung ist ungetestet, da die Modulliste im Mitschnitt leer war.
- Studienablauf/Blockplan: Keine Datenquelle gefunden.

## Tests

Im Projektverzeichnis:

```powershell
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all -- --check
cargo check -p campusunbloat-web --target wasm32-unknown-unknown
```

Die Tests decken unter anderem Prüfungsan- und -abmeldung gegen einen Fake-Server, Prüfungs-Parsing, Sitzungs-Cookies,
Zugriffsschutz, Kalender-Überlappungen, Notendurchschnitt und Mensa-Tageswechsel ab.

## Datenschutz

Benutzername, Passwort und Campus-Dual-Sitzung bleiben im Arbeitsspeicher
des Servers, um eine abgelaufene Campus-Dual-Sitzung erneuern zu können.
Der Browser erhält ein zufälliges, `HttpOnly`-Sitzungs-Cookie (8 Stunden).
Mit "Angemeldet bleiben" kommt ein zweites `HttpOnly`-Cookie (30 Tage) hinzu,
das die Zugangsdaten mit AES-256-GCM verschlüsselt enthält. Der Server
speichert keine Zugangsdaten, nur den Schlüssel: `CAMPUSUNBLOAT_SECRET`
(64 Hex-Zeichen, für Hosting setzen) oder lokal eine automatisch erzeugte
Datei `remember.key` im Datenverzeichnis (`CAMPUSUNBLOAT_DATA_DIR`).
Für HTTPS-Betrieb `CAMPUSUNBLOAT_SECURE_COOKIES=1` setzen. Abmelden löscht
beide Cookies. Die gewählte Mensa wird lokal im Browser gespeichert.

Die Mensa-Anbindung legt im Arbeitsverzeichnis eine `meals.sqlite` als Cache
für öffentliche Speisepläne an. Sie enthält keine Campus-Dual-Zugangsdaten.
Campus-Dual-Anfragen gehen an `idp.dhsn.de` und `fep.campus-dual.de`;
Mensa-Anfragen gehen an das Studierendenwerk Leipzig und OpenMensa.

## Sicherheit

- **Sitzungs-Cookies:** `campus_session` und `campus_remember` sind beide
  `HttpOnly` (kein Zugriff per JavaScript) und `SameSite=Strict`. Lokal ohne
  TLS fehlt zwangsläufig `Secure`; beim Hosting hinter HTTPS unbedingt
  `CAMPUSUNBLOAT_SECURE_COOKIES=1` setzen, sonst würde der Browser die
  Cookies auch über unverschlüsseltes HTTP senden. Ist das Flag gesetzt,
  schickt der Server zusätzlich `Strict-Transport-Security`.
- **CSP:** Die Content-Security-Policy erlaubt nur die beim Build konkret
  erzeugten Inline-Skripte (per SHA-256-Hash), kein `unsafe-inline` für
  Skripte, kein Nachladen von Drittanbieter-Code.
- **Host-/Origin-Prüfung:** Anfragen mit fremdem `Host`-Header werden mit
  403 abgelehnt; schreibende Anfragen (POST) zusätzlich, wenn der
  `Origin`-Header nicht zum eigenen Host passt (CSRF-Schutz). Für Hosting
  unter einer echten Domain muss diese über `CAMPUSUNBLOAT_ALLOWED_HOST`
  freigeschaltet werden (zusätzlich zu `localhost`/`127.0.0.1`, die immer
  erlaubt bleiben).
- **Rate-Limiting:** Login ist auf 10 Versuche pro Minute begrenzt, parallel
  höchstens 64 Sitzungen.
- **Remember-Key:** Die Datei `remember.key` wird unter Unix nur für den
  Eigentümer lesbar angelegt (`chmod 600`), da sie den Schlüssel zum
  Entschlüsseln aller gespeicherten Zugangsdaten enthält. Für Hosting ist
  `CAMPUSUNBLOAT_SECRET` (statt Dateischlüssel) vorzuziehen.
- **Fehlermeldungen:** Der Browser bekommt nur generische Meldungen, Details
  zu Campus-Dual-Fehlern landen ausschließlich im Server-Log.
- **Offene Empfehlung für den Produktivbetrieb:** CampusUnbloatV2 terminiert
  selbst kein TLS, es lauscht nur lokal auf `127.0.0.1`. Für Website/App
  gehört zwingend ein Reverse Proxy mit gültigem Zertifikat davor (z. B.
  Caddy/Nginx), sonst gehen Zugangsdaten unverschlüsselt über das Netz.

Hinweis: Campus Dual selbst setzt sein eigenes Sitzungs-Cookie
(`SAP_SESSIONID_FEP_100`) ohne `HttpOnly`; das betrifft Campus Dual, nicht
CampusUnbloatV2, und lässt sich von hier aus nicht beheben.

## Herkunft und Lizenz

Die Campus-Dual-Anbindung enthält angepasste Teile von
[campus-api](https://github.com/fnschmidt/campus-api). Die Mensa-Anbindung
enthält angepasste Teile von [mensa-api](https://github.com/greybaron/mensa-api).
Beide Projekte stehen unter der GNU General Public License Version 3.
Dieses Projekt wird ebenfalls unter der [GPL-3.0](LICENSE) bereitgestellt.
